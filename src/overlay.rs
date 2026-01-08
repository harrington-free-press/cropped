use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use tracing::info;

use crate::fonts;

/// Add crop marks to a manuscript PDF by expanding pages to A4 and drawing lines.
///
/// Uses a "stamping" approach: the manuscript document is the primary file,
/// preserving its structure, metadata, and page tree. For each manuscript
/// page we:
///
/// - Expand the MediaBox to A4
/// - Wrap the original content in a transformation to center it
/// - Draw crop marks at the trim size corners
///
/// The original manuscript's content streams (i.e. individual pages) are
/// never decoded or modified, minimizing risk of corruption. Crop marks are
/// generated programmatically via native PDF drawing operations.
///
/// The trim size (e.g., 6"×9") defines where crop marks are placed. The actual
/// content may be larger (with bleed) and will be centered accordingly.
pub fn combine(
    output_path: &Path,
    manuscript_path: &Path,
    trim_width: f64,
    trim_height: f64,
) -> lopdf::Result<()> {
    let mut manuscript_document = Document::load(manuscript_path)?;

    info!("Manuscript loaded");

    // Embed font once for all pages
    let font_id = fonts::embed_font(&mut manuscript_document)?;
    info!("Font embedded");

    // Process each manuscript page
    let page_ids: Vec<ObjectId> = manuscript_document.page_iter().collect();
    for (index, page_id) in page_ids.iter().enumerate() {
        stamp_page(&mut manuscript_document, *page_id, trim_width, trim_height, font_id, index + 1)?;
    }

    manuscript_document.compress();

    info!("Save output");
    manuscript_document.save(output_path)?;

    Ok(())
}

/// Generate PDF operations to draw crop marks at the corners of the given
/// content area.
///
/// * `content_x` - Left edge of content area
/// * `content_y` - Bottom edge of content area
/// * `content_width` - Width of content area
/// * `content_height` - Height of content area
///
fn generate_crop_marks(
    content_x: f64,
    content_y: f64,
    content_width: f64,
    content_height: f64,
) -> Vec<Operation> {
    let mut ops = Vec::new();

    // Set crop line width (0.5 pt is reasonably standard)
    ops.push(Operation::new("w", vec![0.5.into()]));

    // Set stroke color to black (in gray scale)
    ops.push(Operation::new("G", vec![0.into()]));

    // Crop mark length extending outside content area
    let mark_length = 20.0;
    let mark_offset = 5.0; // Gap between content edge and crop mark

    // Calculate corner positions
    let left = content_x;
    let right = content_x + content_width;
    let bottom = content_y;
    let top = content_y + content_height;

    // Bottom-left corner (horizontal and vertical marks)
    // Horizontal mark (left side)
    ops.push(Operation::new("m", vec![(left - mark_offset - mark_length).into(), bottom.into()]));
    ops.push(Operation::new("l", vec![(left - mark_offset).into(), bottom.into()]));
    ops.push(Operation::new("S", vec![]));
    // Vertical mark (bottom side)
    ops.push(Operation::new("m", vec![left.into(), (bottom - mark_offset - mark_length).into()]));
    ops.push(Operation::new("l", vec![left.into(), (bottom - mark_offset).into()]));
    ops.push(Operation::new("S", vec![]));

    // Bottom-right corner
    // Horizontal mark (right side)
    ops.push(Operation::new("m", vec![(right + mark_offset).into(), bottom.into()]));
    ops.push(Operation::new("l", vec![(right + mark_offset + mark_length).into(), bottom.into()]));
    ops.push(Operation::new("S", vec![]));
    // Vertical mark (bottom side)
    ops.push(Operation::new("m", vec![right.into(), (bottom - mark_offset - mark_length).into()]));
    ops.push(Operation::new("l", vec![right.into(), (bottom - mark_offset).into()]));
    ops.push(Operation::new("S", vec![]));

    // Top-left corner
    // Horizontal mark (left side)
    ops.push(Operation::new("m", vec![(left - mark_offset - mark_length).into(), top.into()]));
    ops.push(Operation::new("l", vec![(left - mark_offset).into(), top.into()]));
    ops.push(Operation::new("S", vec![]));
    // Vertical mark (top side)
    ops.push(Operation::new("m", vec![left.into(), (top + mark_offset).into()]));
    ops.push(Operation::new("l", vec![left.into(), (top + mark_offset + mark_length).into()]));
    ops.push(Operation::new("S", vec![]));

    // Top-right corner
    // Horizontal mark (right side)
    ops.push(Operation::new("m", vec![(right + mark_offset).into(), top.into()]));
    ops.push(Operation::new("l", vec![(right + mark_offset + mark_length).into(), top.into()]));
    ops.push(Operation::new("S", vec![]));
    // Vertical mark (top side)
    ops.push(Operation::new("m", vec![right.into(), (top + mark_offset).into()]));
    ops.push(Operation::new("l", vec![right.into(), (top + mark_offset + mark_length).into()]));
    ops.push(Operation::new("S", vec![]));

    ops
}

/// Generate PDF operations to draw a page number footer.
///
/// * `page_num` - The page number to display
/// * `page_width` - Width of the page (typically 595 for A4)
/// * `font_name` - The resource name for the font (e.g., "F1")
///
/// The page number is centered horizontally and positioned 1cm (~28.35 pt) from the bottom edge.
fn generate_page_number(page_num: usize, page_width: f64, font_name: &str) -> Vec<Operation> {
    let mut ops = Vec::new();

    // Position 1cm from bottom (28.35 points)
    let y_pos = 28.35;
    let x_pos = page_width / 2.0;

    let text = format!("{}", page_num);

    // Begin text object
    ops.push(Operation::new("BT", vec![]));

    // Set font (Inconsolata at 10pt)
    ops.push(Operation::new("Tf", vec![font_name.into(), 10.into()]));

    // Position text (roughly centered - Inconsolata is monospaced ~6pt per char at 10pt)
    let approx_width = text.len() as f64 * 6.0;
    let centered_x = x_pos - (approx_width / 2.0);
    ops.push(Operation::new("Td", vec![centered_x.into(), y_pos.into()]));

    // Show text
    ops.push(Operation::new("Tj", vec![Object::String(text.into_bytes(), lopdf::StringFormat::Literal)]));

    // End text object
    ops.push(Operation::new("ET", vec![]));

    ops
}

/// Adds crop marks and page number to a single manuscript page.
///
/// Preserves the original page content by wrapping it in transformation
/// operations avoiding the necessity we would otherwise have to decode and
/// re-encode the stream's operations.
///
/// This creates a Contents array:
///
/// [start_wrapper, original_content, end_wrapper]
///
/// where:
///
/// - start_wrapper: crop marks + page number + transformation start (q, cm)
/// - original_content: preserved as-is (Reference or Array)
/// - end_wrapper: transformation end (Q)
///
/// This approach ensures original streams are never modified, minimizing the
/// risk of corrupting the input document's content.
///
/// The trim size defines where crop marks are placed. The actual content (which
/// may include bleed) is read from the original MediaBox and centered accordingly.
fn stamp_page(
    doc: &mut Document,
    page_id: ObjectId,
    trim_width: f64,
    trim_height: f64,
    font_id: ObjectId,
    page_num: usize,
) -> lopdf::Result<()> {
    // Clone the page dictionary once so we can mutate doc
    let page = doc.get_object(page_id)?.as_dict()?.clone();

    // Read original MediaBox to get actual content dimensions
    let original_mediabox = page.get(b"MediaBox")?;
    let (actual_width, actual_height) = match original_mediabox {
        Object::Array(arr) if arr.len() == 4 => {
            // MediaBox format: [x1, y1, x2, y2]
            // Convert to f64 handling both Integer and Real types
            let to_f64 = |obj: &Object| -> lopdf::Result<f64> {
                match obj {
                    Object::Integer(i) => Ok(*i as f64),
                    Object::Real(r) => Ok(*r as f64),
                    _ => Err(lopdf::Error::PageNumberNotFound(0)),
                }
            };
            let x1 = to_f64(&arr[0])?;
            let y1 = to_f64(&arr[1])?;
            let x2 = to_f64(&arr[2])?;
            let y2 = to_f64(&arr[3])?;
            (x2 - x1, y2 - y1)
        }
        _ => return Err(lopdf::Error::PageNumberNotFound(0)),
    };

    let mut new_page = page;

    // Change MediaBox to A4 (595×842)
    new_page.set("MediaBox", vec![0.into(), 0.into(), 595.into(), 842.into()]);

    // Add font to page resources using a unique name
    let font_name = "cropped-F1";

    // Resolve Resources (might be a Reference)
    let resources_obj = new_page.get(b"Resources").ok();
    let res_dict = if let Some(obj) = resources_obj {
        match obj {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(id) => {
                // Dereference the Resources object
                doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()).cloned()
            }
            _ => None,
        }
    } else {
        // No Resources found, create minimal one
        None
    };

    if let Some(mut res_dict) = res_dict {
        // Add or update Font dictionary (handling both direct dict and Reference)
        let font_obj = res_dict.get(b"Font").ok();
        let existing_font_dict = if let Some(obj) = font_obj {
            match obj {
                Object::Dictionary(d) => Some(d.clone()),
                Object::Reference(id) => {
                    // Dereference the Font dictionary
                    doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()).cloned()
                }
                _ => None,
            }
        } else {
            None
        };

        if let Some(mut font_dict) = existing_font_dict {
            font_dict.set(font_name, font_id);
            res_dict.set("Font", font_dict);
        } else {
            let mut font_dict = dictionary! {};
            font_dict.set(font_name, font_id);
            res_dict.set("Font", font_dict);
        }
        new_page.set("Resources", Object::Dictionary(res_dict));
    } else {
        // No existing resources, create new one with just our font
        let resources = dictionary! {
            "Font" => dictionary! {
                font_name => font_id,
            },
        };
        new_page.set("Resources", Object::Dictionary(resources));
    }

    // Center actual content on A4
    let content_x: f64 = (595.0 - actual_width) / 2.0;
    let content_y: f64 = (842.0 - actual_height) / 2.0;

    // Calculate trim area position (centered on A4)
    let trim_x: f64 = (595.0 - trim_width) / 2.0;
    let trim_y: f64 = (842.0 - trim_height) / 2.0;

    // Create wrapper stream: crop marks + page number + transformation start
    let mut start_ops = Vec::new();
    // Draw crop marks at trim size position
    start_ops.extend(generate_crop_marks(trim_x, trim_y, trim_width, trim_height));
    // Draw page number
    start_ops.extend(generate_page_number(page_num, 595.0, font_name));
    start_ops.push(Operation::new("q", vec![]));
    start_ops.push(Operation::new("cm", vec![
        1.into(),
        0.into(),
        0.into(),
        1.into(),
        content_x.into(),
        content_y.into(),
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

    // Replace page in document
    doc.objects.insert(page_id, Object::Dictionary(new_page));

    Ok(())
}
