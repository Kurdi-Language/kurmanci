#![allow(non_camel_case_types)]
#![allow(clippy::missing_safety_doc)]

use kurmanci_engine::{
    CompletionOptions, CorrectionOptions, EngineError, KurmanciEngine, PackLoadError,
    PredictionOptions, SuggestOptions, SuggestionKind,
};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

pub const KMR_ABI_VERSION_MAJOR: u32 = 1;
pub const KMR_ABI_VERSION_MINOR: u32 = 0;

pub type kmr_status = u32;

pub const KMR_OK: kmr_status = 0;
pub const KMR_ERROR_INVALID_ARGUMENT: kmr_status = 1;
pub const KMR_ERROR_IO: kmr_status = 2;
pub const KMR_ERROR_INVALID_PACK: kmr_status = 3;
pub const KMR_ERROR_UNSUPPORTED_PACK: kmr_status = 4;
pub const KMR_ERROR_INCOMPATIBLE_LANGUAGE: kmr_status = 5;
pub const KMR_ERROR_CHECKSUM: kmr_status = 6;
pub const KMR_ERROR_INTERNAL: kmr_status = 7;

pub type kmr_suggestion_kind = u32;

pub const KMR_SUGGESTION_EXACT: kmr_suggestion_kind = 0;
pub const KMR_SUGGESTION_COMPLETION: kmr_suggestion_kind = 1;
pub const KMR_SUGGESTION_CORRECTION: kmr_suggestion_kind = 2;
pub const KMR_SUGGESTION_DIACRITIC_CORRECTION: kmr_suggestion_kind = 3;
pub const KMR_SUGGESTION_NEXT_WORD: kmr_suggestion_kind = 4;

pub type kmr_prediction_source = u32;

pub const KMR_PREDICTION_TRIGRAM: kmr_prediction_source = 0;
pub const KMR_PREDICTION_BIGRAM_BACKOFF: kmr_prediction_source = 1;
pub const KMR_PREDICTION_BIGRAM: kmr_prediction_source = 2;
pub const KMR_PREDICTION_NONE: kmr_prediction_source = 3;

thread_local! {
    static LAST_ERROR_MESSAGE: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(msg: &str) {
    let c_str =
        CString::new(msg).unwrap_or_else(|_| CString::new("Invalid error message").unwrap());
    LAST_ERROR_MESSAGE.with(|cell| {
        *cell.borrow_mut() = c_str;
    });
}

fn clear_last_error() {
    LAST_ERROR_MESSAGE.with(|cell| {
        *cell.borrow_mut() = CString::new("").unwrap();
    });
}

#[derive(Debug)]
pub enum FfiError {
    InvalidArgument(String),
    Engine(EngineError),
    Internal(String),
}

fn map_engine_error(err: EngineError) -> (kmr_status, String) {
    let msg = err.to_string();
    let status = match err {
        EngineError::Io(_) => KMR_ERROR_IO,
        EngineError::PackLoad(ref pack_err) => match pack_err {
            PackLoadError::TooShort(_) => KMR_ERROR_INVALID_PACK,
            PackLoadError::InvalidMagicBytes => KMR_ERROR_INVALID_PACK,
            PackLoadError::UnsupportedVersion { .. } => KMR_ERROR_UNSUPPORTED_PACK,
            PackLoadError::IncompatibleLanguage { .. } => KMR_ERROR_INCOMPATIBLE_LANGUAGE,
            PackLoadError::ChecksumMismatch => KMR_ERROR_CHECKSUM,
            PackLoadError::TruncatedPayload => KMR_ERROR_INVALID_PACK,
            PackLoadError::InvalidPayload { .. } => KMR_ERROR_INVALID_PACK,
        },
    };
    (status, msg)
}

fn ffi_guard<F>(f: F) -> kmr_status
where
    F: FnOnce() -> Result<(), FfiError>,
{
    clear_last_error();
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => KMR_OK,
        Ok(Err(FfiError::InvalidArgument(msg))) => {
            set_last_error(&msg);
            KMR_ERROR_INVALID_ARGUMENT
        }
        Ok(Err(FfiError::Engine(err))) => {
            let (status, msg) = map_engine_error(err);
            set_last_error(&msg);
            status
        }
        Ok(Err(FfiError::Internal(msg))) => {
            set_last_error(&msg);
            KMR_ERROR_INTERNAL
        }
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                format!("Panic caught: {}", s)
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                format!("Panic caught: {}", s)
            } else {
                "Panic caught: unknown internal panic".to_string()
            };
            set_last_error(&msg);
            KMR_ERROR_INTERNAL
        }
    }
}

pub struct EngineHandle {
    engine: KurmanciEngine,
    language_tag: CString,
}

#[repr(C)]
pub struct kmr_engine {
    _private: [u8; 0],
}

#[repr(C)]
pub struct kmr_suggestion_list {
    items: Vec<OwnedSuggestionItem>,
}

pub struct OwnedSuggestionItem {
    text: CString,
    kind: kmr_suggestion_kind,
    edit_cost: u32,
}

#[repr(C)]
pub struct kmr_prediction_list {
    items: Vec<OwnedPredictionItem>,
}

pub struct OwnedPredictionItem {
    text: CString,
    count: u64,
    probability_millionths: u32,
    source: kmr_prediction_source,
}

#[repr(C)]
pub struct kmr_pack_info {
    pub language_tag: *const c_char,
    pub format_version: u32,
    pub entry_count: usize,
}

#[repr(C)]
pub struct kmr_suggestion_item {
    pub text: *const c_char,
    pub kind: kmr_suggestion_kind,
    pub edit_cost: u32,
}

#[repr(C)]
pub struct kmr_prediction_item {
    pub text: *const c_char,
    pub count: u64,
    pub probability_millionths: u32,
    pub source: kmr_prediction_source,
}

fn parse_c_str(ptr: *const c_char, name: &str) -> Result<&str, FfiError> {
    if ptr.is_null() {
        return Err(FfiError::InvalidArgument(format!(
            "Null pointer passed for {}",
            name
        )));
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| FfiError::InvalidArgument(format!("Invalid UTF-8 string passed for {}", name)))
}

fn to_c_string(value: String) -> Result<CString, FfiError> {
    CString::new(value)
        .map_err(|_| FfiError::Internal("engine result contained an interior NUL byte".to_string()))
}

#[doc(hidden)]
pub fn test_panic_guard() -> kmr_status {
    ffi_guard(|| panic!("test panic containment"))
}

#[no_mangle]
pub extern "C" fn kmr_abi_version_major() -> u32 {
    KMR_ABI_VERSION_MAJOR
}

#[no_mangle]
pub extern "C" fn kmr_abi_version_minor() -> u32 {
    KMR_ABI_VERSION_MINOR
}

#[no_mangle]
pub unsafe extern "C" fn kmr_last_error_message() -> *const c_char {
    LAST_ERROR_MESSAGE.with(|cell| cell.borrow().as_ptr())
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_create_from_file(
    path_utf8: *const c_char,
    out_engine: *mut *mut kmr_engine,
) -> kmr_status {
    if !out_engine.is_null() {
        *out_engine = std::ptr::null_mut();
    }
    ffi_guard(|| {
        if out_engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_engine".to_string(),
            ));
        }
        let path_str = parse_c_str(path_utf8, "path_utf8")?;
        let engine =
            KurmanciEngine::from_pack_file(PathBuf::from(path_str)).map_err(FfiError::Engine)?;
        let lang_tag = CString::new(engine.pack_info().language_tag.clone())
            .map_err(|_| FfiError::Internal("Invalid language tag".to_string()))?;

        let handle = Box::new(EngineHandle {
            engine,
            language_tag: lang_tag,
        });
        *out_engine = Box::into_raw(handle) as *mut kmr_engine;
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_create_from_bytes(
    data: *const u8,
    length: usize,
    out_engine: *mut *mut kmr_engine,
) -> kmr_status {
    if !out_engine.is_null() {
        *out_engine = std::ptr::null_mut();
    }
    ffi_guard(|| {
        if out_engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_engine".to_string(),
            ));
        }
        if data.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null data pointer passed".to_string(),
            ));
        }
        let bytes = std::slice::from_raw_parts(data, length);
        let engine = KurmanciEngine::from_pack_bytes(bytes).map_err(FfiError::Engine)?;
        let lang_tag = CString::new(engine.pack_info().language_tag.clone())
            .map_err(|_| FfiError::Internal("Invalid language tag".to_string()))?;

        let handle = Box::new(EngineHandle {
            engine,
            language_tag: lang_tag,
        });
        *out_engine = Box::into_raw(handle) as *mut kmr_engine;
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_destroy(engine: *mut kmr_engine) {
    if engine.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = Box::from_raw(engine as *mut EngineHandle);
    }));
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_get_info(
    engine: *const kmr_engine,
    out_info: *mut kmr_pack_info,
) -> kmr_status {
    if !out_info.is_null() {
        std::ptr::write_bytes(out_info, 0, 1);
    }
    ffi_guard(|| {
        if engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for engine".to_string(),
            ));
        }
        if out_info.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_info".to_string(),
            ));
        }
        let handle = &*(engine as *const EngineHandle);
        let info = handle.engine.pack_info();
        *out_info = kmr_pack_info {
            language_tag: handle.language_tag.as_ptr(),
            format_version: info.format_version,
            entry_count: info.entry_count,
        };
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_is_known_word(
    engine: *const kmr_engine,
    word_utf8: *const c_char,
    out_is_known: *mut bool,
) -> kmr_status {
    if !out_is_known.is_null() {
        *out_is_known = false;
    }
    ffi_guard(|| {
        if engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for engine".to_string(),
            ));
        }
        if out_is_known.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_is_known".to_string(),
            ));
        }
        let word_str = parse_c_str(word_utf8, "word_utf8")?;
        let handle = &*(engine as *const EngineHandle);
        *out_is_known = handle.engine.is_known_word(word_str);
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_correct(
    engine: *const kmr_engine,
    input_utf8: *const c_char,
    limit: usize,
    out_results: *mut *mut kmr_suggestion_list,
) -> kmr_status {
    if !out_results.is_null() {
        *out_results = std::ptr::null_mut();
    }
    ffi_guard(|| {
        if engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for engine".to_string(),
            ));
        }
        if out_results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_results".to_string(),
            ));
        }
        let input_str = parse_c_str(input_utf8, "input_utf8")?;
        let handle = &*(engine as *const EngineHandle);
        let results = handle
            .engine
            .correct(input_str, CorrectionOptions { limit });

        let items = results
            .into_iter()
            .map(|r| {
                Ok(OwnedSuggestionItem {
                    text: to_c_string(r.text)?,
                    kind: match r.kind {
                        SuggestionKind::Exact => KMR_SUGGESTION_EXACT,
                        SuggestionKind::Completion => KMR_SUGGESTION_COMPLETION,
                        SuggestionKind::Correction => KMR_SUGGESTION_CORRECTION,
                        SuggestionKind::DiacriticCorrection => KMR_SUGGESTION_DIACRITIC_CORRECTION,
                        SuggestionKind::NextWord => KMR_SUGGESTION_NEXT_WORD,
                    },
                    edit_cost: r.edit_cost,
                })
            })
            .collect::<Result<Vec<_>, FfiError>>()?;

        *out_results = Box::into_raw(Box::new(kmr_suggestion_list { items }));
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_complete(
    engine: *const kmr_engine,
    prefix_utf8: *const c_char,
    limit: usize,
    out_results: *mut *mut kmr_suggestion_list,
) -> kmr_status {
    if !out_results.is_null() {
        *out_results = std::ptr::null_mut();
    }
    ffi_guard(|| {
        if engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for engine".to_string(),
            ));
        }
        if out_results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_results".to_string(),
            ));
        }
        let prefix_str = parse_c_str(prefix_utf8, "prefix_utf8")?;
        let handle = &*(engine as *const EngineHandle);
        let results = handle
            .engine
            .complete(prefix_str, CompletionOptions { limit });

        let items = results
            .into_iter()
            .map(|r| {
                Ok(OwnedSuggestionItem {
                    text: to_c_string(r.text)?,
                    kind: match r.kind {
                        SuggestionKind::Exact => KMR_SUGGESTION_EXACT,
                        SuggestionKind::Completion => KMR_SUGGESTION_COMPLETION,
                        SuggestionKind::Correction => KMR_SUGGESTION_CORRECTION,
                        SuggestionKind::DiacriticCorrection => KMR_SUGGESTION_DIACRITIC_CORRECTION,
                        SuggestionKind::NextWord => KMR_SUGGESTION_NEXT_WORD,
                    },
                    edit_cost: r.edit_cost,
                })
            })
            .collect::<Result<Vec<_>, FfiError>>()?;

        *out_results = Box::into_raw(Box::new(kmr_suggestion_list { items }));
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_suggest(
    engine: *const kmr_engine,
    input_utf8: *const c_char,
    limit: usize,
    out_results: *mut *mut kmr_suggestion_list,
) -> kmr_status {
    if !out_results.is_null() {
        *out_results = std::ptr::null_mut();
    }
    ffi_guard(|| {
        if engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for engine".to_string(),
            ));
        }
        if out_results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_results".to_string(),
            ));
        }
        let input_str = parse_c_str(input_utf8, "input_utf8")?;
        let handle = &*(engine as *const EngineHandle);
        let results = handle.engine.suggest(input_str, SuggestOptions { limit });

        let items = results
            .into_iter()
            .map(|r| {
                Ok(OwnedSuggestionItem {
                    text: to_c_string(r.text)?,
                    kind: match r.kind {
                        SuggestionKind::Exact => KMR_SUGGESTION_EXACT,
                        SuggestionKind::Completion => KMR_SUGGESTION_COMPLETION,
                        SuggestionKind::Correction => KMR_SUGGESTION_CORRECTION,
                        SuggestionKind::DiacriticCorrection => KMR_SUGGESTION_DIACRITIC_CORRECTION,
                        SuggestionKind::NextWord => KMR_SUGGESTION_NEXT_WORD,
                    },
                    edit_cost: r.edit_cost,
                })
            })
            .collect::<Result<Vec<_>, FfiError>>()?;

        *out_results = Box::into_raw(Box::new(kmr_suggestion_list { items }));
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_engine_predict_next(
    engine: *const kmr_engine,
    context_words_utf8: *const *const c_char,
    context_count: usize,
    limit: usize,
    out_results: *mut *mut kmr_prediction_list,
) -> kmr_status {
    if !out_results.is_null() {
        *out_results = std::ptr::null_mut();
    }
    ffi_guard(|| {
        if engine.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for engine".to_string(),
            ));
        }
        if out_results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_results".to_string(),
            ));
        }
        if context_count > 0 && context_words_utf8.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null context_words_utf8 pointer passed with context_count > 0".to_string(),
            ));
        }

        let mut context_strs = Vec::with_capacity(context_count);
        if context_count > 0 {
            let slice = std::slice::from_raw_parts(context_words_utf8, context_count);
            for &ptr in slice {
                let s = parse_c_str(ptr, "context_words_utf8")?;
                context_strs.push(s);
            }
        }

        let handle = &*(engine as *const EngineHandle);
        let results = handle
            .engine
            .predict_next(&context_strs, PredictionOptions { limit });

        let items = results
            .into_iter()
            .map(|p| {
                Ok(OwnedPredictionItem {
                    text: to_c_string(p.text)?,
                    count: p.count,
                    probability_millionths: p.probability_millionths,
                    source: match p.source {
                        kurmanci_engine::PredictionSource::Trigram => KMR_PREDICTION_TRIGRAM,
                        kurmanci_engine::PredictionSource::BigramBackoff => {
                            KMR_PREDICTION_BIGRAM_BACKOFF
                        }
                        kurmanci_engine::PredictionSource::Bigram => KMR_PREDICTION_BIGRAM,
                        kurmanci_engine::PredictionSource::None => KMR_PREDICTION_NONE,
                    },
                })
            })
            .collect::<Result<Vec<_>, FfiError>>()?;

        *out_results = Box::into_raw(Box::new(kmr_prediction_list { items }));
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_suggestion_list_len(
    results: *const kmr_suggestion_list,
    out_len: *mut usize,
) -> kmr_status {
    if !out_len.is_null() {
        *out_len = 0;
    }
    ffi_guard(|| {
        if results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for results".to_string(),
            ));
        }
        if out_len.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_len".to_string(),
            ));
        }
        let list = &*results;
        *out_len = list.items.len();
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_suggestion_list_get(
    results: *const kmr_suggestion_list,
    index: usize,
    out_item: *mut kmr_suggestion_item,
) -> kmr_status {
    if !out_item.is_null() {
        std::ptr::write_bytes(out_item, 0, 1);
    }
    ffi_guard(|| {
        if results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for results".to_string(),
            ));
        }
        if out_item.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_item".to_string(),
            ));
        }
        let list = &*results;
        if index >= list.items.len() {
            return Err(FfiError::InvalidArgument(format!(
                "Index {} out of bounds (len {})",
                index,
                list.items.len()
            )));
        }
        let item = &list.items[index];
        *out_item = kmr_suggestion_item {
            text: item.text.as_ptr(),
            kind: item.kind,
            edit_cost: item.edit_cost,
        };
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_suggestion_list_destroy(results: *mut kmr_suggestion_list) {
    if results.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = Box::from_raw(results);
    }));
}

#[no_mangle]
pub unsafe extern "C" fn kmr_prediction_list_len(
    results: *const kmr_prediction_list,
    out_len: *mut usize,
) -> kmr_status {
    if !out_len.is_null() {
        *out_len = 0;
    }
    ffi_guard(|| {
        if results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for results".to_string(),
            ));
        }
        if out_len.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_len".to_string(),
            ));
        }
        let list = &*results;
        *out_len = list.items.len();
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_prediction_list_get(
    results: *const kmr_prediction_list,
    index: usize,
    out_item: *mut kmr_prediction_item,
) -> kmr_status {
    if !out_item.is_null() {
        std::ptr::write_bytes(out_item, 0, 1);
    }
    ffi_guard(|| {
        if results.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for results".to_string(),
            ));
        }
        if out_item.is_null() {
            return Err(FfiError::InvalidArgument(
                "Null pointer passed for out_item".to_string(),
            ));
        }
        let list = &*results;
        if index >= list.items.len() {
            return Err(FfiError::InvalidArgument(format!(
                "Index {} out of bounds (len {})",
                index,
                list.items.len()
            )));
        }
        let item = &list.items[index];
        *out_item = kmr_prediction_item {
            text: item.text.as_ptr(),
            count: item.count,
            probability_millionths: item.probability_millionths,
            source: item.source,
        };
        Ok(())
    })
}

#[no_mangle]
pub unsafe extern "C" fn kmr_prediction_list_destroy(results: *mut kmr_prediction_list) {
    if results.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = Box::from_raw(results);
    }));
}
