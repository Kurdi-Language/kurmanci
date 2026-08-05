use kurmanci_ffi::*;
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::PathBuf;

fn get_pack_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs")
        .join(name)
        .join("lexicon.bin")
}

#[test]
fn test_c_abi_version_and_error_message() {
    assert_eq!(kmr_abi_version_major(), 1);
    assert_eq!(kmr_abi_version_minor(), 0);

    let err_ptr = unsafe { kmr_last_error_message() };
    assert!(!err_ptr.is_null());
    let err_str = unsafe { std::ffi::CStr::from_ptr(err_ptr).to_str().unwrap() };
    assert_eq!(err_str, "");
}

#[test]
fn test_c_abi_engine_handle_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EngineHandle>();
}

#[test]
fn test_c_abi_null_pointer_validation_and_out_param_zeroing() {
    unsafe {
        let mut engine_ptr: *mut kmr_engine = 0x12345 as *mut kmr_engine;
        let status = kmr_engine_create_from_file(std::ptr::null(), &mut engine_ptr);
        assert_eq!(status, KMR_ERROR_INVALID_ARGUMENT);
        assert!(engine_ptr.is_null());

        let status2 = kmr_engine_create_from_bytes(std::ptr::null(), 0, &mut engine_ptr);
        assert_eq!(status2, KMR_ERROR_INVALID_ARGUMENT);
        assert!(engine_ptr.is_null());

        let mut is_known = true;
        let status3 = kmr_engine_is_known_word(std::ptr::null(), std::ptr::null(), &mut is_known);
        assert_eq!(status3, KMR_ERROR_INVALID_ARGUMENT);
        assert!(!is_known);

        let mut results_ptr: *mut kmr_suggestion_list = 0x54321 as *mut kmr_suggestion_list;
        let status4 = kmr_engine_suggest(std::ptr::null(), std::ptr::null(), 5, &mut results_ptr);
        assert_eq!(status4, KMR_ERROR_INVALID_ARGUMENT);
        assert!(results_ptr.is_null());
    }
}

#[test]
fn test_c_abi_invalid_utf8_handling() {
    unsafe {
        let bad_bytes = [0xFFu8, 0xFE, 0xFD, 0x00];
        let bad_c_str = bad_bytes.as_ptr() as *const c_char;

        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let mut engine_ptr: *mut kmr_engine = std::ptr::null_mut();
        let status = kmr_engine_create_from_file(c_path.as_ptr(), &mut engine_ptr);
        assert_eq!(status, KMR_OK);
        assert!(!engine_ptr.is_null());

        let mut is_known = false;
        let status_word = kmr_engine_is_known_word(engine_ptr, bad_c_str, &mut is_known);
        assert_eq!(status_word, KMR_ERROR_INVALID_ARGUMENT);
        assert!(!is_known);

        let mut results: *mut kmr_suggestion_list = std::ptr::null_mut();
        let status_sug = kmr_engine_suggest(engine_ptr, bad_c_str, 5, &mut results);
        assert_eq!(status_sug, KMR_ERROR_INVALID_ARGUMENT);
        assert!(results.is_null());

        kmr_engine_destroy(engine_ptr);
    }
}

#[test]
fn test_c_abi_pack_load_error_code_mappings() {
    unsafe {
        let mut engine_ptr: *mut kmr_engine = std::ptr::null_mut();

        // 1. Too Short
        let short_bytes = [1u8, 2, 3];
        let s1 =
            kmr_engine_create_from_bytes(short_bytes.as_ptr(), short_bytes.len(), &mut engine_ptr);
        assert_eq!(s1, KMR_ERROR_INVALID_PACK);

        // 2. Invalid Magic Bytes
        let mut bad_magic = [0u8; 32];
        bad_magic[0..4].copy_from_slice(b"BADM");
        let s2 = kmr_engine_create_from_bytes(bad_magic.as_ptr(), bad_magic.len(), &mut engine_ptr);
        assert_eq!(s2, KMR_ERROR_INVALID_PACK);

        // 3. Unsupported Version
        let mut bad_version = [0u8; 32];
        bad_version[0..4].copy_from_slice(b"KRM1");
        bad_version[4..8].copy_from_slice(&99u32.to_le_bytes());
        let s3 =
            kmr_engine_create_from_bytes(bad_version.as_ptr(), bad_version.len(), &mut engine_ptr);
        assert_eq!(s3, KMR_ERROR_UNSUPPORTED_PACK);

        // 4. Incompatible Language
        let mut bad_lang = [0u8; 32];
        bad_lang[0..4].copy_from_slice(b"KRM1");
        bad_lang[4..8].copy_from_slice(&4u32.to_le_bytes());
        bad_lang[8..10].copy_from_slice(&2u16.to_le_bytes());
        bad_lang[10..12].copy_from_slice(b"en");
        let s4 = kmr_engine_create_from_bytes(bad_lang.as_ptr(), bad_lang.len(), &mut engine_ptr);
        assert_eq!(s4, KMR_ERROR_INCOMPATIBLE_LANGUAGE);

        // 5. Checksum Mismatch
        let seed_path = get_pack_path("seed");
        let mut corrupted = std::fs::read(&seed_path).expect("seed pack file must exist");
        if let Some(last) = corrupted.last_mut() {
            *last ^= 0xFF;
        }
        let s5 = kmr_engine_create_from_bytes(corrupted.as_ptr(), corrupted.len(), &mut engine_ptr);
        assert_eq!(s5, KMR_ERROR_CHECKSUM);
    }
}

#[test]
fn test_c_abi_list_out_of_bounds_access() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_file(c_path.as_ptr(), &mut engine),
            KMR_OK
        );

        let c_query = CString::new("spaz").unwrap();
        let mut suggestions: *mut kmr_suggestion_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_correct(engine, c_query.as_ptr(), 5, &mut suggestions),
            KMR_OK
        );

        let mut len = 0;
        assert_eq!(kmr_suggestion_list_len(suggestions, &mut len), KMR_OK);
        assert!(len > 0);

        let mut item = kmr_suggestion_item {
            text: std::ptr::null(),
            kind: 0,
            edit_cost: 0,
        };
        let status_oob = kmr_suggestion_list_get(suggestions, 9999, &mut item);
        assert_eq!(status_oob, KMR_ERROR_INVALID_ARGUMENT);
        assert!(item.text.is_null());

        kmr_suggestion_list_destroy(suggestions);
        kmr_engine_destroy(engine);
    }
}

#[test]
fn test_c_abi_repeated_create_query_destroy_memory_cycle() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let c_query = CString::new("roj").unwrap();
        let c_word = CString::new("welat").unwrap();

        for _ in 0..100 {
            let mut engine: *mut kmr_engine = std::ptr::null_mut();
            assert_eq!(
                kmr_engine_create_from_file(c_path.as_ptr(), &mut engine),
                KMR_OK
            );

            let mut is_known = false;
            assert_eq!(
                kmr_engine_is_known_word(engine, c_word.as_ptr(), &mut is_known),
                KMR_OK
            );
            assert!(is_known);

            let mut completions: *mut kmr_suggestion_list = std::ptr::null_mut();
            assert_eq!(
                kmr_engine_complete(engine, c_query.as_ptr(), 5, &mut completions),
                KMR_OK
            );

            let mut len = 0;
            assert_eq!(kmr_suggestion_list_len(completions, &mut len), KMR_OK);
            assert!(len > 0);

            kmr_suggestion_list_destroy(completions);
            kmr_engine_destroy(engine);
        }
    }
}

#[test]
fn test_c_abi_create_from_bytes_success() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let bytes = std::fs::read(&seed_path).expect("seed pack file must exist");
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_bytes(bytes.as_ptr(), bytes.len(), &mut engine),
            KMR_OK
        );
        assert!(!engine.is_null());

        let mut info = kmr_pack_info {
            language_tag: std::ptr::null(),
            format_version: 0,
            entry_count: 0,
        };
        assert_eq!(kmr_engine_get_info(engine, &mut info), KMR_OK);
        assert_eq!(info.format_version, 4);
        assert_eq!(info.entry_count, 33);

        kmr_engine_destroy(engine);
    }
}

#[test]
fn test_c_abi_suggest() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_file(c_path.as_ptr(), &mut engine),
            KMR_OK
        );

        let c_input = CString::new("spaz").unwrap();
        let mut suggestions: *mut kmr_suggestion_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_suggest(engine, c_input.as_ptr(), 5, &mut suggestions),
            KMR_OK
        );
        assert!(!suggestions.is_null());

        let mut len = 0;
        assert_eq!(kmr_suggestion_list_len(suggestions, &mut len), KMR_OK);
        assert!(len > 0);

        let mut item = kmr_suggestion_item {
            text: std::ptr::null(),
            kind: 0,
            edit_cost: 0,
        };
        assert_eq!(kmr_suggestion_list_get(suggestions, 0, &mut item), KMR_OK);
        assert!(!item.text.is_null());
        let text_str = std::ffi::CStr::from_ptr(item.text).to_str().unwrap();
        assert_eq!(text_str, "spas");

        kmr_suggestion_list_destroy(suggestions);
        kmr_engine_destroy(engine);
    }
}

#[test]
fn test_c_abi_prediction_inspection() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_file(c_path.as_ptr(), &mut engine),
            KMR_OK
        );

        let w1 = CString::new("ez").unwrap();
        let w2 = CString::new("diçim").unwrap();
        let context = [w1.as_ptr(), w2.as_ptr()];

        let mut predictions: *mut kmr_prediction_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_predict_next(engine, context.as_ptr(), 2, 5, &mut predictions),
            KMR_OK
        );
        assert!(!predictions.is_null());

        let mut len = 0;
        assert_eq!(kmr_prediction_list_len(predictions, &mut len), KMR_OK);
        // Seed pack model profile is none, so length is 0
        assert_eq!(len, 0);

        kmr_prediction_list_destroy(predictions);
        kmr_engine_destroy(engine);
    }
}

#[test]
fn test_c_abi_limits() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_file(c_path.as_ptr(), &mut engine),
            KMR_OK
        );

        let c_input = CString::new("spaz").unwrap();

        // Limit 0 returns empty list
        let mut empty_results: *mut kmr_suggestion_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_correct(engine, c_input.as_ptr(), 0, &mut empty_results),
            KMR_OK
        );
        let mut len0 = 999;
        assert_eq!(kmr_suggestion_list_len(empty_results, &mut len0), KMR_OK);
        assert_eq!(len0, 0);
        kmr_suggestion_list_destroy(empty_results);

        // Large limit succeeds and is clamped to max 50
        let mut large_results: *mut kmr_suggestion_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_correct(engine, c_input.as_ptr(), 100, &mut large_results),
            KMR_OK
        );
        let mut len_large = 0;
        assert_eq!(
            kmr_suggestion_list_len(large_results, &mut len_large),
            KMR_OK
        );
        assert!(len_large > 0 && len_large <= 50);
        kmr_suggestion_list_destroy(large_results);

        kmr_engine_destroy(engine);
    }
}

#[test]
fn test_c_abi_null_context_validation() {
    unsafe {
        let seed_path = get_pack_path("seed");
        let c_path = CString::new(seed_path.to_str().unwrap()).unwrap();
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_file(c_path.as_ptr(), &mut engine),
            KMR_OK
        );

        // context_words_utf8 == NULL with context_count == 0 must succeed
        let mut predictions: *mut kmr_prediction_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_predict_next(engine, std::ptr::null(), 0, 5, &mut predictions),
            KMR_OK
        );
        assert!(!predictions.is_null());
        kmr_prediction_list_destroy(predictions);

        // context_words_utf8 == NULL with context_count > 0 must fail with KMR_ERROR_INVALID_ARGUMENT
        let mut bad_predictions: *mut kmr_prediction_list = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_predict_next(engine, std::ptr::null(), 2, 5, &mut bad_predictions),
            KMR_ERROR_INVALID_ARGUMENT
        );
        assert!(bad_predictions.is_null());

        kmr_engine_destroy(engine);
    }
}

#[test]
fn test_c_abi_panic_containment() {
    let status = test_panic_guard();
    assert_eq!(status, KMR_ERROR_INTERNAL);

    let err_ptr = unsafe { kmr_last_error_message() };
    assert!(!err_ptr.is_null());
    let err_str = unsafe { std::ffi::CStr::from_ptr(err_ptr).to_str().unwrap() };
    assert!(err_str.contains("test panic containment"));
}
