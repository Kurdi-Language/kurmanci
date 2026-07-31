pub mod hunspell;

pub use hunspell::{
    import_hunspell_dic, map_part_of_speech, parse_hunspell_line, parse_hunspell_source,
    ConflictRecord, HunspellSourceEvent, ImportSummaryReport, ImportedLexiconRecord,
    ParsedHunspellEntry, RejectedRecord,
};
