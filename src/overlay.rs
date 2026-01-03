use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

/// Combines a manuscript PDF with a crop marks template.
/// Each page of the manuscript is stamped onto a template page.
pub fn combine(
    template_path: &Path,
    manuscript_path: &Path,
    output_path: &Path,
) -> lopdf::Result<()> {
    let template = Document::load(template_path)?;
    let manuscript = Document::load(manuscript_path)?;

    let mut output = Document::with_version("1.7");

    // The template with the crop marks is a single page. We get the first
    // page of the template.
    let template_page_id = template
        .page_iter()
        .next()
        .ok_or(lopdf::Error::ObjectNotFound((0, 0)))?;

    // For each manuscript page, create a new combined page
    let mut new_page_ids = Vec::new();

    for manuscript_page_id in manuscript.page_iter() {
        let new_page_id = combine_page(
            &template,
            &manuscript,
            template_page_id,
            manuscript_page_id,
            &mut output,
        )?;
        new_page_ids.push(new_page_id);
    }

    // Create new Pages root object
    let pages_id = output.new_object_id();

    // Update all pages to have correct Parent reference
    for page_id in &new_page_ids {
        if let Ok(Object::Dictionary(mut page_dict)) =
            output.get_object_mut(*page_id).map(|o| o.clone())
        {
            page_dict.set("Parent", pages_id);
            output
                .objects
                .insert(*page_id, Object::Dictionary(page_dict));
        }
    }

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => new_page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
        "Count" => new_page_ids.len() as u32,
    };

    output.objects.insert(pages_id, Object::Dictionary(pages));

    // Create Catalog
    let catalog_id = output.new_object_id();

    let catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };

    output
        .objects
        .insert(catalog_id, Object::Dictionary(catalog));
    output.trailer.set("Root", catalog_id);

    // Copy Info dictionary from manuscript if it exists
    if let Ok(info_ref) = manuscript.trailer.get(b"Info") {
        if let Object::Reference(info_id) = info_ref {
            if let Ok(info_obj) = manuscript.get_object(*info_id) {
                let new_info_id = output.add_object(info_obj.clone());
                output.trailer.set("Info", new_info_id);
            }
        }
    }

    output.compress();

    output.save(output_path)?;

    Ok(())
}

fn combine_page(
    template: &Document,
    manuscript: &Document,
    template_page_id: ObjectId,
    manuscript_page_id: ObjectId,
    output: &mut Document,
) -> lopdf::Result<ObjectId> {
    // Get template page dictionary
    let template_page = template.get_object(template_page_id)?.as_dict()?;

    // Start with template page as base
    let mut new_page_dict = Dictionary::new();
    new_page_dict.set("Type", "Page");

    // Copy MediaBox from template (A4 size)
    if let Ok(media_box) = template_page.get(b"MediaBox") {
        new_page_dict.set("MediaBox", media_box.clone());
    }

    // Get or create resources, starting with template resources
    let mut resources = get_resources_dict(template, template_page)?;

    // Create a Form XObject from the manuscript page
    let xobject_name = b"ManuscriptPage".to_vec();
    let form_xobject_id = create_form_xobject(manuscript, manuscript_page_id, output)?;

    // Add the Form XObject to resources
    let mut xobjects = if let Ok(xobj_dict) = resources.get(b"XObject") {
        if let Object::Dictionary(dict) = xobj_dict {
            dict.clone()
        } else {
            Dictionary::new()
        }
    } else {
        Dictionary::new()
    };

    xobjects.set(xobject_name.clone(), Object::Reference(form_xobject_id));
    resources.set("XObject", Object::Dictionary(xobjects));

    // Build combined content stream
    let mut operations = Vec::new();

    // First, draw template content (crop marks)
    if let Ok(content_ref) = template_page.get(b"Contents") {
        operations.extend(get_content_operations(template, content_ref)?);
    }

    // Then, stamp the manuscript page on top
    // The manuscript page is 6"×9" = 432×648 points
    // A4 is 595×842 points
    // Center it: (595-432)/2 = 81.5, (842-648)/2 = 97

    operations.push(Operation::new("q", vec![])); // Save graphics state
    operations.push(Operation::new(
        "cm",
        vec![
            1.into(),
            0.into(),
            0.into(),
            1.into(),
            81.5.into(),
            97.into(),
        ],
    ));
    operations.push(Operation::new("Do", vec![Object::Name(xobject_name)]));
    operations.push(Operation::new("Q", vec![])); // Restore graphics state

    // Create new content stream
    let content = Content { operations };
    let content_id = output.add_object(Stream::new(dictionary! {}, content.encode()?));

    // Add resources to output
    let resources_id = output.add_object(Object::Dictionary(resources));

    // Build new page
    new_page_dict.set("Contents", content_id);
    new_page_dict.set("Resources", resources_id);

    let page_id = output.add_object(Object::Dictionary(new_page_dict));

    Ok(page_id)
}

fn create_form_xobject(
    manuscript: &Document,
    page_id: ObjectId,
    output: &mut Document,
) -> lopdf::Result<ObjectId> {
    let page = manuscript.get_object(page_id)?.as_dict()?;

    // Get page content
    let content_ops = if let Ok(content_ref) = page.get(b"Contents") {
        get_content_operations(manuscript, content_ref)?
    } else {
        Vec::new()
    };

    // Get page resources and copy them to output
    let resources = get_resources_dict(manuscript, page)?;
    let resources_id = copy_resources_deep(manuscript, &resources, output)?;

    // Get MediaBox for BBox
    let bbox = if let Ok(media_box) = page.get(b"MediaBox") {
        media_box.clone()
    } else {
        // Default to 6"×9" in points (432×648)
        vec![0.into(), 0.into(), 432.into(), 648.into()].into()
    };

    // Create Form XObject
    let content = Content {
        operations: content_ops,
    };
    let form_dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => bbox,
        "Resources" => resources_id,
    };

    let form_stream = Stream::new(form_dict, content.encode()?);
    let form_id = output.add_object(form_stream);

    Ok(form_id)
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
    match content_ref {
        Object::Reference(content_id) => {
            let content_obj = doc.get_object(*content_id)?;
            if let Ok(stream) = content_obj.as_stream() {
                Ok(Content::decode(&stream.content)?.operations)
            } else {
                Ok(Vec::new())
            }
        }
        Object::Array(arr) => {
            // Contents can be an array of streams
            let mut all_ops = Vec::new();
            for item in arr {
                if let Object::Reference(id) = item {
                    let obj = doc.get_object(*id)?;
                    if let Ok(stream) = obj.as_stream() {
                        all_ops.extend(Content::decode(&stream.content)?.operations);
                    }
                }
            }
            Ok(all_ops)
        }
        _ => Ok(Vec::new()),
    }
}

fn copy_resources_deep(
    source: &Document,
    resources: &Dictionary,
    output: &mut Document,
) -> lopdf::Result<ObjectId> {
    let mut new_resources = Dictionary::new();

    for (key, value) in resources.iter() {
        let new_value = copy_object_deep(source, value, output)?;
        new_resources.set(key.clone(), new_value);
    }

    let res_id = output.add_object(Object::Dictionary(new_resources));

    Ok(res_id)
}

fn copy_object_deep(
    source: &Document,
    obj: &Object,
    output: &mut Document,
) -> lopdf::Result<Object> {
    match obj {
        Object::Reference(id) => {
            // Dereference and copy the actual object
            let referenced_obj = source.get_object(*id)?;
            match referenced_obj {
                Object::Stream(stream) => {
                    let new_id = output.add_object(Object::Stream(stream.clone()));
                    Ok(Object::Reference(new_id))
                }
                Object::Dictionary(dict) => {
                    let mut new_dict = Dictionary::new();
                    for (k, v) in dict.iter() {
                        let new_v = copy_object_deep(source, v, output)?;
                        new_dict.set(k.clone(), new_v);
                    }
                    let new_id = output.add_object(Object::Dictionary(new_dict));
                    Ok(Object::Reference(new_id))
                }
                _ => {
                    let new_id = output.add_object(referenced_obj.clone());
                    Ok(Object::Reference(new_id))
                }
            }
        }
        Object::Dictionary(dict) => {
            let mut new_dict = Dictionary::new();
            for (k, v) in dict.iter() {
                let new_v = copy_object_deep(source, v, output)?;
                new_dict.set(k.clone(), new_v);
            }
            Ok(Object::Dictionary(new_dict))
        }
        Object::Array(arr) => {
            let mut new_arr = Vec::new();
            for item in arr {
                let new_item = copy_object_deep(source, item, output)?;
                new_arr.push(new_item);
            }
            Ok(Object::Array(new_arr))
        }
        _ => Ok(obj.clone()),
    }
}
