use std::path::Path;
use std::collections::HashMap;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use tracing::info;

/// Combines a manuscript PDF with a crop marks template.
/// The template crop marks are stamped onto each manuscript page.
pub fn combine(
    template_path: &Path,
    output_path: &Path,
    manuscript_path: &Path,
) -> lopdf::Result<()> {
    let template = Document::load(template_path)?;
    let mut manuscript = Document::load(manuscript_path)?;

    info!("Files loaded");

    // Get the template page (crop marks)
    let template_page_id = template
        .page_iter()
        .next()
        .ok_or(lopdf::Error::ObjectNotFound((0, 0)))?;
    let template_page = template.get_object(template_page_id)?.as_dict()?;

    // Get template content operations
    let template_ops = if let Ok(content_ref) = template_page.get(b"Contents") {
        get_content_operations(&template, content_ref)?
    } else {
        Vec::new()
    };

    // Copy template resources once
    let template_resources = get_resources_dict(&template, template_page)?;
    let mut cache: HashMap<ObjectId, ObjectId> = HashMap::new();
    let template_resources_copy = copy_resources(&template, &template_resources, &mut manuscript, &mut cache)?;

    // Process each manuscript page
    let page_ids: Vec<ObjectId> = manuscript.page_iter().collect();
    for page_id in page_ids {
        stamp_page(
            &mut manuscript,
            page_id,
            &template_ops,
            &template_resources_copy,
        )?;
    }

    manuscript.compress();
    info!("Save output");
    manuscript.save(output_path)?;

    Ok(())
}

fn stamp_page(
    doc: &mut Document,
    page_id: ObjectId,
    template_ops: &[Operation],
    template_resources: &Dictionary,
) -> lopdf::Result<()> {
    let page = doc.get_object(page_id)?.as_dict()?.clone();

    // Change MediaBox from 6"×9" (432×648) to A4 (595×842)
    let mut new_page = page.clone();
    new_page.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);

    // Get existing page content
    let page_ops = if let Ok(content_ref) = page.get(b"Contents") {
        get_content_operations(doc, content_ref)?
    } else {
        Vec::new()
    };

    // Build new content: template first (underneath), then manuscript content centered
    let mut ops = Vec::new();

    // Draw template (crop marks)
    ops.extend(template_ops.iter().cloned());

    // Center manuscript content with coordinate transformation
    // Typst uses a flipped Y coordinate system, so we need to flip and position
    // Transform: flip Y axis and translate to center
    ops.push(Operation::new("q", vec![]));
    ops.push(Operation::new("cm", vec![
        1.into(),
        0.into(),
        0.into(),
        (-1).into(),        // Flip Y-axis
        81.5.into(),        // Horizontal centering: (595-432)/2
        (97.0 + 648.0).into(), // Vertical: bottom_margin + height
    ]));
    ops.extend(page_ops);
    ops.push(Operation::new("Q", vec![]));

    // Create new content stream
    let content = Content { operations: ops };
    let content_data = content.encode()?;
    let content_stream = Stream::new(dictionary! {}, content_data);
    let new_content_id = doc.add_object(content_stream);
    new_page.set("Contents", new_content_id);

    // Merge resources - create a new merged dictionary and add as new object
    let page_resources = get_resources_dict(doc, &page)?;
    let merged_resources = merge_resources(&page_resources, template_resources);
    let resources_ref = doc.add_object(Object::Dictionary(merged_resources));
    new_page.set("Resources", resources_ref);

    // Replace page in document
    doc.objects.insert(page_id, Object::Dictionary(new_page));

    Ok(())
}

fn copy_resources(
    source: &Document,
    resources: &Dictionary,
    dest: &mut Document,
    cache: &mut HashMap<ObjectId, ObjectId>,
) -> lopdf::Result<Dictionary> {
    let mut new_resources = Dictionary::new();

    for (key, value) in resources.iter() {
        let new_value = copy_object_deep(source, value, dest, cache)?;
        new_resources.set(key.clone(), new_value);
    }

    Ok(new_resources)
}

fn merge_resources(base: &Dictionary, overlay: &Dictionary) -> Dictionary {
    let mut merged = base.clone();

    for (key, value) in overlay.iter() {
        match base.get(key) {
            Ok(base_value) => {
                // Both have this resource type - need to merge the subdictionaries
                match (base_value, value) {
                    (Object::Reference(_base_ref), Object::Dictionary(_overlay_dict)) => {
                        // Base is a reference - we need to keep it as a reference but can't merge easily
                        // For now, just add overlay items with unique names
                        // This is a limitation but avoids breaking existing references
                        merged.set(key.clone(), base_value.clone());
                    }
                    (Object::Dictionary(base_dict), Object::Dictionary(overlay_dict)) => {
                        // Both are dictionaries - merge them
                        let mut merged_dict = base_dict.clone();
                        for (k, v) in overlay_dict.iter() {
                            // Only add if not already present to avoid conflicts
                            if merged_dict.get(k).is_err() {
                                merged_dict.set(k.clone(), v.clone());
                            }
                        }
                        merged.set(key.clone(), Object::Dictionary(merged_dict));
                    }
                    _ => {
                        // Keep base value for other cases
                        merged.set(key.clone(), base_value.clone());
                    }
                }
            }
            Err(_) => {
                // Key doesn't exist in base, just add overlay value
                merged.set(key.clone(), value.clone());
            }
        }
    }

    merged
}

fn get_resources_dict(doc: &Document, page: &Dictionary) -> lopdf::Result<Dictionary> {
    if let Ok(res_ref) = page.get(b"Resources") {
        match res_ref {
            Object::Reference(res_id) => Ok(doc.get_object(*res_id)?.as_dict()?.clone()),
            Object::Dictionary(dict) => Ok(dict.clone()),
            _ => Ok(Dictionary::new()),
        }
    } else {
        Ok(Dictionary::new())
    }
}

fn get_content_operations(doc: &Document, content_ref: &Object) -> lopdf::Result<Vec<Operation>> {
    // Note it is critical that we use decompressed_content() in each of these
    // cases as we can't (and don't need to) anticipate whether or not the
    // input PDF elements are compressed.
    match content_ref {
        Object::Reference(content_id) => {
            let content_obj = doc.get_object(*content_id)?;
            if let Ok(stream) = content_obj.as_stream() {
                let decoded = stream.decompressed_content()?;
                Ok(Content::decode(&decoded)?.operations)
            } else {
                Ok(Vec::new())
            }
        }
        Object::Array(arr) => {
            let mut operations = Vec::new();
            for item in arr {
                if let Object::Reference(id) = item {
                    let obj = doc.get_object(*id)?;
                    if let Ok(stream) = obj.as_stream() {
                        let decoded = stream.decompressed_content()?;
                        operations.extend(Content::decode(&decoded)?.operations);
                    }
                }
            }
            Ok(operations)
        }
        _ => Ok(Vec::new()),
    }
}

fn copy_object_deep(
    source: &Document,
    obj: &Object,
    output: &mut Document,
    cache: &mut HashMap<ObjectId, ObjectId>,
) -> lopdf::Result<Object> {
    match obj {
        Object::Reference(id) => {
            // Check cache first to avoid duplicating objects
            if let Some(&cached_id) = cache.get(id) {
                return Ok(Object::Reference(cached_id));
            }

            // Dereference and copy the actual object
            let referenced_obj = source.get_object(*id)?;
            let new_id = match referenced_obj {
                Object::Stream(stream) => {
                    // For streams (including images), recursively copy all dictionary values
                    // to properly handle ColorSpace, SMask, and other referenced objects
                    let mut new_dict = Dictionary::new();

                    for (k, v) in stream.dict.iter() {
                        let new_v = copy_object_deep(source, v, output, cache)?;
                        new_dict.set(k.clone(), new_v);
                    }

                    let new_stream = Stream::new(new_dict, stream.content.clone());
                    output.add_object(Object::Stream(new_stream))
                }
                Object::Dictionary(dict) => {
                    let mut new_dict = Dictionary::new();
                    for (k, v) in dict.iter() {
                        let new_v = copy_object_deep(source, v, output, cache)?;
                        new_dict.set(k.clone(), new_v);
                    }
                    output.add_object(Object::Dictionary(new_dict))
                }
                _ => {
                    output.add_object(referenced_obj.clone())
                }
            };

            // Cache the mapping
            cache.insert(*id, new_id);
            Ok(Object::Reference(new_id))
        }
        Object::Dictionary(dict) => {
            let mut new_dict = Dictionary::new();
            for (k, v) in dict.iter() {
                let new_v = copy_object_deep(source, v, output, cache)?;
                new_dict.set(k.clone(), new_v);
            }
            Ok(Object::Dictionary(new_dict))
        }
        Object::Array(arr) => {
            let mut new_arr = Vec::new();
            for item in arr {
                let new_item = copy_object_deep(source, item, output, cache)?;
                new_arr.push(new_item);
            }
            Ok(Object::Array(new_arr))
        }
        _ => Ok(obj.clone()),
    }
}
