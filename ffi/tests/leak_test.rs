//! Resident-process memory behaviour across the C boundary: repeated create/query/destroy
//! cycles and long query loops on one handle must return the live heap exactly to its
//! baseline, so no result list, string or handle leaks. A counting allocator is installed
//! in this test binary; it is the only test in the file so the counters are undisturbed.

use kurmanci_ffi::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            LIVE.fetch_add(new_size, Ordering::Relaxed);
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

unsafe fn query_cycle(engine: *mut kmr_engine, input: &CString) -> usize {
    let mut total = 0usize;
    let mut known = false;
    assert_eq!(
        kmr_engine_is_known_word(engine, input.as_ptr(), &mut known),
        KMR_OK
    );
    total += usize::from(known);
    for f in [kmr_engine_suggest, kmr_engine_correct, kmr_engine_complete] {
        let mut list: *mut kmr_suggestion_list = std::ptr::null_mut();
        assert_eq!(f(engine, input.as_ptr(), 5, &mut list), KMR_OK);
        let mut len = 0usize;
        assert_eq!(kmr_suggestion_list_len(list, &mut len), KMR_OK);
        for i in 0..len {
            let mut item = kmr_suggestion_item {
                text: std::ptr::null(),
                kind: 0,
                edit_cost: 0,
            };
            assert_eq!(kmr_suggestion_list_get(list, i, &mut item), KMR_OK);
            total += item.text.is_null() as usize;
        }
        kmr_suggestion_list_destroy(list);
        total += len;
    }
    let ctx = [input.as_ptr()];
    let mut preds: *mut kmr_prediction_list = std::ptr::null_mut();
    assert_eq!(
        kmr_engine_predict_next(engine, ctx.as_ptr(), 1, 5, &mut preds),
        KMR_OK
    );
    let mut len = 0usize;
    assert_eq!(kmr_prediction_list_len(preds, &mut len), KMR_OK);
    kmr_prediction_list_destroy(preds);
    total + len
}

#[test]
fn create_query_destroy_cycles_and_long_query_loops_do_not_leak() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/build/packs/seed/lexicon.bin");
    let bytes = std::fs::read(&path).expect("seed pack must be built for FFI tests");
    let inputs: Vec<CString> = [
        "welat",
        "spaz",
        "roj",
        "rojb",
        "bas\u{0327}",
        "ÇAV",
        "xyz",
        "welat\u{200B}",
        "ez",
    ]
    .iter()
    .map(|s| CString::new(*s).unwrap())
    .collect();

    // Warm up: the thread-local last-error string is allocated on first error and kept.
    unsafe {
        let bad: &[u8] = b"\xFF\x00";
        let mut known = false;
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_bytes(bytes.as_ptr(), bytes.len(), &mut engine),
            KMR_OK
        );
        assert_eq!(
            kmr_engine_is_known_word(engine, bad.as_ptr() as *const c_char, &mut known),
            KMR_ERROR_INVALID_ARGUMENT
        );
        for input in &inputs {
            query_cycle(engine, input);
        }
        kmr_engine_destroy(engine);
    }

    // 1. Create / query / destroy cycles return to baseline.
    let baseline = live();
    let mut total = 0usize;
    for _ in 0..500 {
        unsafe {
            let mut engine: *mut kmr_engine = std::ptr::null_mut();
            assert_eq!(
                kmr_engine_create_from_bytes(bytes.as_ptr(), bytes.len(), &mut engine),
                KMR_OK
            );
            for input in &inputs {
                total += query_cycle(engine, input);
            }
            kmr_engine_destroy(engine);
        }
    }
    assert_eq!(
        live(),
        baseline,
        "create/query/destroy cycles leaked {} bytes",
        live() as isize - baseline as isize
    );

    // 2. A long query loop on one resident handle, including failing calls, stays flat.
    unsafe {
        let mut engine: *mut kmr_engine = std::ptr::null_mut();
        assert_eq!(
            kmr_engine_create_from_bytes(bytes.as_ptr(), bytes.len(), &mut engine),
            KMR_OK
        );
        for input in &inputs {
            total += query_cycle(engine, input);
        }
        let resident = live();
        let bad: &[u8] = b"welat\xFF\x00";
        for i in 0..20_000 {
            total += query_cycle(engine, &inputs[i % inputs.len()]);
            if i % 100 == 0 {
                let mut list: *mut kmr_suggestion_list = std::ptr::null_mut();
                assert_eq!(
                    kmr_engine_suggest(engine, bad.as_ptr() as *const c_char, 5, &mut list),
                    KMR_ERROR_INVALID_ARGUMENT
                );
                assert!(list.is_null());
            }
        }
        assert_eq!(
            live(),
            resident,
            "resident query loop leaked {} bytes",
            live() as isize - resident as isize
        );
        kmr_engine_destroy(engine);
    }
    assert_eq!(live(), baseline, "destroy must release the engine");
    assert!(total > 0);
}
