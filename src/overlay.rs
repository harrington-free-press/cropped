use std::path::Path;
use std::collections::HashMap;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use tracing::info;

/// Combines a manuscript PDF with a crop marks template.
///
/// Uses a "stamping" approach where the manuscript document is the primary file,
/// preserving its structure, metadata, and page tree. For each manuscript page:
/// - Expands MediaBox from 6"×9" to A4
/// - Wraps original content in transformation to center it
/// - Prepends template crop marks (drawn underneath)
///
/// Original manuscript content streams are never decoded or modified, minimizing
/// risk of corruption. Template resources are copied once and shared across all pages.
pub fn combine(
    template_path: &Path,
    output_path: &Path,
    manuscript_path: &Path,
) -> lopdf::Result<()> {
    let template_document = Document::load(template_path)?;
    let mut manuscript_document = Document::load(manuscript_path)?;

    info!("Files loaded");

    // Get the template page (crop marks) - single page document
    let template_page_id = template_document
        .page_iter()
        .next()
        .ok_or(lopdf::Error::ObjectNotFound((0, 0)))?;
    let template_page = template_document.get_object(template_page_id)?.as_dict()?;

    // Decode template content operations (the drawing commands for the crop
    // marks).
    let template_ops = if let Ok(content_ref) = template_page.get(b"Contents") {
        get_content_operations(&template_document, content_ref)?
    } else {
        Vec::new()
    };

    // They are, however, resources in the template document and not in the
    // new one. Translate object references from one to the other. The
    // resulting Dictionary contains References valid in the output
    // manuscript, pointing to shared objects (fonts, graphics, etc.) that
    // will be reused across all pages.
    let template_resources = get_resources_dict(&template_document, template_page)?;
    let mut cache: HashMap<ObjectId, ObjectId> = HashMap::new();

    // Copy the resources to become references into the the new document (the
    // one based on the manuscript)
    let template_resources = copy_resources(&template_document, &template_resources, &mut manuscript_document, &mut cache)?;

    // Process each manuscript page
    let page_ids: Vec<ObjectId> = manuscript_document.page_iter().collect();
    for page_id in page_ids {
        stamp_page(
            &mut manuscript_document,
            page_id,
            &template_ops,
            &template_resources,
        )?;
    }

    manuscript_document.compress();
    info!("Save output");
    manuscript_document.save(output_path)?;

    Ok(())
}

/// Stamps the template onto a single manuscript page.
///
/// Preserves the original page content by wrapping it in transformation operations
/// rather than decoding and re-encoding. Creates a Contents array:
/// [start_wrapper, original_content, end_wrapper] where:
/// - start_wrapper: template crop marks + transformation start (q, cm)
/// - original_content: preserved as-is (Reference or Array)
/// - end_wrapper: transformation end (Q)
///
/// This approach ensures original content streams are never modified, minimizing
/// corruption risk. The template_resources Dictionary is passed by reference as
/// it contains References to shared objects already copied into this document.
fn stamp_page(
    doc: &mut Document,
    page_id: ObjectId,
    template_ops: &[Operation],
    template_resources: &Dictionary,
) -> lopdf::Result<()> {
    // Clone the page dictionary once so we can mutate doc
    let mut new_page = doc.get_object(page_id)?.as_dict()?.clone();

    // Change MediaBox from 6"×9" (432×648) to A4 (595×842)
    new_page.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);

    // Create wrapper stream: template + transformation start
    let mut start_ops = Vec::new();
    start_ops.extend(template_ops.iter().cloned());
    start_ops.push(Operation::new("q", vec![]));
    start_ops.push(Operation::new("cm", vec![
        1.into(),
        0.into(),
        0.into(),
        (-1).into(),        // Flip Y-axis
        81.5.into(),        // Horizontal centering: (595-432)/2
        (97.0 + 648.0).into(), // Vertical: bottom_margin + height
    ]));

    let start_content = Content { operations: start_ops };
    let start_stream = Stream::new(dictionary! {}, start_content.encode()?);
    let start_id = doc.add_object(start_stream);

    // Create wrapper stream: transformation end
    let end_ops = vec![Operation::new("Q", vec![])];
    let end_content = Content { operations: end_ops };
    let end_stream = Stream::new(dictionary! {}, end_content.encode()?);
    let end_id = doc.add_object(end_stream);

    // Build Contents array preserving original content objects
    // Per PDF spec, Contents can be a single stream Reference or an Array of References.
    // We convert to Array format to sandwich the original content between our wrappers.
    // This is harmless - viewers simply concatenate streams in order.
    let mut contents_array = vec![Object::Reference(start_id)];

    if let Ok(original_contents) = new_page.get(b"Contents") {
        match original_contents {
            Object::Reference(_) => {
                // Single stream (typical case for Typst PDFs) - add as-is
                contents_array.push(original_contents.clone());
            }
            Object::Array(arr) => {
                // Already an array - preserve all elements
                contents_array.extend(arr.iter().cloned());
            }
            _ => {
                // Unexpected type, but handle gracefully (blank page)
            }
        }
    }

    contents_array.push(Object::Reference(end_id));
    new_page.set("Contents", Object::Array(contents_array));

    // Merge resources - combine page's existing resources with template resources
    // Each manuscript page has different resources (fonts, images, etc.), so we must
    // merge per-page. Note that cloning dictionaries copies the HashMap structure,
    // byte vector keys, and nested dictionaries, though the actual PDF objects
    // (fonts, images) are referenced, not duplicated.
    let page_resources = get_resources_dict(doc, &new_page)?;
    let merged_resources = merge_resources(&page_resources, template_resources);
    let resources_ref = doc.add_object(Object::Dictionary(merged_resources));
    new_page.set("Resources", resources_ref);

    // Replace page in document
    doc.objects.insert(page_id, Object::Dictionary(new_page));

    Ok(())
}

/// Copies resources from source document to destination document.
///
/// The resources Dictionary typically contains References to objects in the source
/// document (e.g., Font: Reference(789)). These References are meaningless in the
/// destination document. This function:
/// 1. Dereferences objects in the source document
/// 2. Deep copies them to the destination document (getting new IDs)
/// 3. Returns a Dictionary with References pointing to the newly copied objects
///
/// The cache prevents duplicating shared objects - if multiple resources reference
/// the same font or color space, it's only copied once.
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

/// Merges two resource dictionaries, preferring base values in case of conflicts.
///
/// Resources are structured as dictionaries of dictionaries, e.g.:
/// { "Font": { "F1": Reference(123), "F2": Reference(456) } }
///
/// When merging, we combine the subdictionaries, adding overlay entries only if
/// they don't conflict with base entries. This preserves the manuscript's resources
/// while adding template resources.
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

/// Recursively copies an object from source document to output document.
///
/// Handles all PDF object types, following References to copy the actual objects.
/// The cache ensures shared objects (like color spaces referenced by multiple images)
/// are only copied once - subsequent references return a Reference to the cached copy.
///
/// This is critical for:
/// - Avoiding duplicate objects in output (keeping file size reasonable)
/// - Preserving object relationships (multiple references to same object stay that way)
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
