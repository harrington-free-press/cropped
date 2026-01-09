# Add Crop Marks to a Manuscript

Place each page of a PDF manuscript in trade format (6 inches × 9 inches) into
an A4 format page (210 mm × 297 mm) with crop marks at each corner. The goal
is an accurate camera-ready representation of the source PDF document that can
be printed on A4 paper for review or editing.

The program to do the combining is written in Rust using the **lopdf** library
to do the low-level PDF operations necessary to access each page of the input
source, translate it to the middle of the A4 page, and then merge it with a
template containing the crop marks and footer. This binary, _cropped_ can be
built with

    $ cargo build
    $ cargo run -- --help

as you would expect of a Rust program. You specify the output filename with
`-o` and then supply the filename of the manuscript you wish to print.

    $ cropped -o Output.pdf Input.pdf

The resultant PDF will have the timestamp, input filename, and page number as
shown in this example:

![Example Screenshot](images/Screenshot.png)

PDF metadata from the input document is preserved; the embellished output
still has the same title, keywords, and other Document Information Dictionary
fields.

Of note is that the page number shown in the footer is the absolute document
page number, not the one that might be typset on the page according to its
position in the frontmatter or main body. This facilitates accurately
selecting subranges of the document for printing when reviewing or editing.
