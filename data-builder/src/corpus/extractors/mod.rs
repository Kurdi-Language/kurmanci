pub mod wikimedia;

pub use wikimedia::{
    extract_wikimedia_file, extract_wikimedia_xml, strip_wikitext, ExtractedWikiDocument,
    WikimediaExtractionReport,
};
