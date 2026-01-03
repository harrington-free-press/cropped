use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use tracing::info;

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

    // Process each manuscript page
    let page_ids: Vec<ObjectId> = manuscript_document.page_iter().collect();
    for page_id in page_ids {
        stamp_page(&mut manuscript_document, page_id, trim_width, trim_height)?;
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

/// Adds crop marks to a single manuscript page.
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
/// - start_wrapper: crop marks + transformation start (q, cm)
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

    // Center actual content on A4
    let content_x: f64 = (595.0 - actual_width) / 2.0;
    let content_y: f64 = (842.0 - actual_height) / 2.0;

    // Calculate trim area position (centered on A4)
    let trim_x: f64 = (595.0 - trim_width) / 2.0;
    let trim_y: f64 = (842.0 - trim_height) / 2.0;

    // Create wrapper stream: crop marks + transformation start
    let mut start_ops = Vec::new();
    // Draw crop marks at trim size position
    start_ops.extend(generate_crop_marks(trim_x, trim_y, trim_width, trim_height));
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
