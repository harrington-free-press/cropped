use std::fs;

use lopdf::{Document, ObjectId, Stream, dictionary};

const FONT_PATH: &str = "/usr/share/fonts/levien-inconsolata/Inconsolata-Regular.ttf";

/// Embed a TrueType font into the PDF document.
///
/// Creates the necessary font descriptor, font file stream, and font
/// dictionary objects required for PDF font embedding. Returns the ObjectId
/// of the font dictionary and the width of a monospaced character, in points,
/// at 1pt font size.
pub fn embed_font(doc: &mut Document) -> lopdf::Result<(ObjectId, f64)> {
    let font_data = fs::read(FONT_PATH).map_err(|e| lopdf::Error::IO(e))?;

    let face =
        ttf_parser::Face::parse(&font_data, 0).map_err(|_| lopdf::Error::PageNumberNotFound(0))?;

    // Extract metrics
    let bbox = face.global_bounding_box();
    let ascender = face.ascender();
    let descender = face.descender();
    let cap_height = face.capital_height().unwrap_or(700);

    // Get character width for monospaced font (use '0' as representative glyph)
    let units_per_em = face.units_per_em() as f64;
    let char_width   = face
        .glyph_index('0')
        .and_then(|glyph_id| face.glyph_hor_advance(glyph_id))
        .map(|advance| advance as f64 / units_per_em)
        .unwrap_or(0.6);

    // Create font file stream (clone font_data since face still borrows it)
    let font_stream = Stream::new(
        dictionary! {
            "Length1" => (font_data.len() as i64),
        },
        font_data.clone(),
    );
    let font_stream_id = doc.add_object(font_stream);

    // Create font descriptor
    let font_descriptor = dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "Inconsolata-Regular",
        "Flags" => 32, // Symbolic font
        "FontBBox" => vec![
            (bbox.x_min as i64).into(),
            (bbox.y_min as i64).into(),
            (bbox.x_max as i64).into(),
            (bbox.y_max as i64).into(),
        ],
        "ItalicAngle" => 0,
        "Ascent" => (ascender as i64),
        "Descent" => (descender as i64),
        "CapHeight" => (cap_height as i64),
        "StemV" => 80,
        "FontFile2" => font_stream_id,
    };
    let font_descriptor_id = doc.add_object(font_descriptor);

    // Create font dictionary
    let font_dict = dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => "Inconsolata-Regular",
        "FontDescriptor" => font_descriptor_id,
        "Encoding" => "WinAnsiEncoding",
    };
    let font_id = doc.add_object(font_dict);

    Ok((font_id, char_width))
}
