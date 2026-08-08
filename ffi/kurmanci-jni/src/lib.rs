use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jboolean, jint, jlong, jobject, jobjectArray, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use kurmanci_ffi::{
    kmr_engine_complete, kmr_engine_correct, kmr_engine_create_from_bytes,
    kmr_engine_create_from_file, kmr_engine_destroy, kmr_engine_get_info, kmr_engine_is_known_word,
    kmr_engine_predict_next, kmr_engine_suggest, kmr_last_error_message, kmr_pack_info,
    kmr_prediction_item, kmr_prediction_list_destroy, kmr_prediction_list_get,
    kmr_prediction_list_len, kmr_suggestion_item, kmr_suggestion_list, kmr_suggestion_list_destroy,
    kmr_suggestion_list_get, kmr_suggestion_list_len, KMR_OK,
};
use std::ffi::{CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn get_last_error_string() -> String {
    unsafe {
        let ptr = kmr_last_error_message();
        if ptr.is_null() {
            "Unknown native error".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

fn throw_kmr_exception(env: &mut JNIEnv, status: u32) {
    let msg = get_last_error_string();
    let class_name = match status {
        1 => "org/kurmanci/KurmanciException$InvalidArgumentException",
        2 => "org/kurmanci/KurmanciException$IoException",
        3 => "org/kurmanci/KurmanciException$InvalidPackException",
        4 => "org/kurmanci/KurmanciException$IncompatiblePackException",
        5 => "org/kurmanci/KurmanciException$IncompatiblePackException",
        6 => "org/kurmanci/KurmanciException$InvalidPackException",
        _ => "org/kurmanci/KurmanciException$NativeException",
    };
    let _ = env.throw_new(class_name, format!("[status {}] {}", status, msg));
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeCreate(
    mut env: JNIEnv,
    _class: JClass,
    pack_data: JByteArray,
) -> jlong {
    let res = catch_unwind(AssertUnwindSafe(|| {
        let bytes = match env.convert_byte_array(&pack_data) {
            Ok(b) => b,
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    format!("Failed to read byte array: {}", e),
                );
                return 0i64;
            }
        };

        let mut out_engine = std::ptr::null_mut();
        let status =
            unsafe { kmr_engine_create_from_bytes(bytes.as_ptr(), bytes.len(), &mut out_engine) };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return 0i64;
        }

        out_engine as jlong
    }));

    match res {
        Ok(h) => h,
        Err(_) => {
            let _ = env.throw_new(
                "org/kurmanci/KurmanciException$NativeException",
                "Internal panic during engine creation",
            );
            0i64
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeCreatePath(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jlong {
    let res = catch_unwind(AssertUnwindSafe(|| {
        let path_str: String = match env.get_string(&path) {
            Ok(s) => s.into(),
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    format!("Invalid path string: {}", e),
                );
                return 0i64;
            }
        };

        let c_path = match CString::new(path_str) {
            Ok(c) => c,
            Err(_) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    "Path contains NUL byte",
                );
                return 0i64;
            }
        };

        let mut out_engine = std::ptr::null_mut();
        let status = unsafe { kmr_engine_create_from_file(c_path.as_ptr(), &mut out_engine) };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return 0i64;
        }

        out_engine as jlong
    }));

    match res {
        Ok(h) => h,
        Err(_) => {
            let _ = env.throw_new(
                "org/kurmanci/KurmanciException$NativeException",
                "Internal panic during engine file creation",
            );
            0i64
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        kmr_engine_destroy(handle as *mut _);
    }));
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeGetPackInfo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jobject {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "KurmanciEngine handle is closed",
        );
        return std::ptr::null_mut();
    }

    let res = catch_unwind(AssertUnwindSafe(|| {
        let mut info: kmr_pack_info = unsafe { std::mem::zeroed() };
        let status = unsafe { kmr_engine_get_info(handle as *const _, &mut info) };
        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return std::ptr::null_mut();
        }

        let lang_tag = unsafe {
            if info.language_tag.is_null() {
                ""
            } else {
                CStr::from_ptr(info.language_tag).to_str().unwrap_or("")
            }
        };

        let j_lang = match env.new_string(lang_tag) {
            Ok(s) => s,
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$NativeException",
                    format!("Failed to create language tag string: {}", e),
                );
                return std::ptr::null_mut();
            }
        };

        let pack_info_class = match env.find_class("org/kurmanci/PackInfo") {
            Ok(c) => c,
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$NativeException",
                    format!("Class org.kurmanci.PackInfo not found: {}", e),
                );
                return std::ptr::null_mut();
            }
        };

        let obj = match env.new_object(
            pack_info_class,
            "(Ljava/lang/String;IJ)V",
            &[
                (&j_lang).into(),
                (info.format_version as jint).into(),
                (info.entry_count as jlong).into(),
            ],
        ) {
            Ok(o) => o,
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$NativeException",
                    format!("Failed to construct PackInfo: {}", e),
                );
                return std::ptr::null_mut();
            }
        };

        obj.into_raw()
    }));

    match res {
        Ok(ptr) => ptr,
        Err(_) => {
            let _ = env.throw_new(
                "org/kurmanci/KurmanciException$NativeException",
                "Panic during nativeGetPackInfo",
            );
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeIsKnownWord(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    word: JString,
) -> jboolean {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "KurmanciEngine handle is closed",
        );
        return JNI_FALSE;
    }

    let res = catch_unwind(AssertUnwindSafe(|| {
        let word_str: String = match env.get_string(&word) {
            Ok(s) => s.into(),
            Err(_) => return JNI_FALSE,
        };
        let c_word = match CString::new(word_str) {
            Ok(c) => c,
            Err(_) => return JNI_FALSE,
        };

        let mut is_known = false;
        let status =
            unsafe { kmr_engine_is_known_word(handle as *const _, c_word.as_ptr(), &mut is_known) };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return JNI_FALSE;
        }

        if is_known {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }));

    match res {
        Ok(b) => b,
        Err(_) => JNI_FALSE,
    }
}

fn convert_suggestion_list(env: &mut JNIEnv, list_ptr: *mut kmr_suggestion_list) -> jobjectArray {
    if list_ptr.is_null() {
        return std::ptr::null_mut();
    }

    let mut len: usize = 0;
    let status = unsafe { kmr_suggestion_list_len(list_ptr, &mut len) };
    if status != KMR_OK {
        unsafe { kmr_suggestion_list_destroy(list_ptr) };
        throw_kmr_exception(env, status);
        return std::ptr::null_mut();
    }

    let candidate_class = match env.find_class("org/kurmanci/Candidate") {
        Ok(c) => c,
        Err(e) => {
            unsafe { kmr_suggestion_list_destroy(list_ptr) };
            let _ = env.throw_new(
                "org/kurmanci/KurmanciException$NativeException",
                format!("Class org.kurmanci.Candidate not found: {}", e),
            );
            return std::ptr::null_mut();
        }
    };

    let arr = match env.new_object_array(len as jint, &candidate_class, JObject::null()) {
        Ok(a) => a,
        Err(e) => {
            unsafe { kmr_suggestion_list_destroy(list_ptr) };
            let _ = env.throw_new(
                "org/kurmanci/KurmanciException$NativeException",
                format!("Failed to allocate Candidate array: {}", e),
            );
            return std::ptr::null_mut();
        }
    };

    for i in 0..len {
        let mut item: kmr_suggestion_item = unsafe { std::mem::zeroed() };
        let st = unsafe { kmr_suggestion_list_get(list_ptr, i, &mut item) };
        if st != KMR_OK {
            unsafe { kmr_suggestion_list_destroy(list_ptr) };
            throw_kmr_exception(env, st);
            return std::ptr::null_mut();
        }

        let text_str = unsafe {
            if item.text.is_null() {
                ""
            } else {
                CStr::from_ptr(item.text).to_str().unwrap_or("")
            }
        };
        let j_text = match env.new_string(text_str) {
            Ok(s) => s,
            Err(_) => {
                unsafe { kmr_suggestion_list_destroy(list_ptr) };
                return std::ptr::null_mut();
            }
        };

        let obj = match env.new_object(
            &candidate_class,
            "(Ljava/lang/String;II)V",
            &[
                (&j_text).into(),
                (item.kind as jint).into(),
                (item.edit_cost as jint).into(),
            ],
        ) {
            Ok(o) => o,
            Err(_) => {
                unsafe { kmr_suggestion_list_destroy(list_ptr) };
                return std::ptr::null_mut();
            }
        };

        if env.set_object_array_element(&arr, i as jint, &obj).is_err() {
            unsafe { kmr_suggestion_list_destroy(list_ptr) };
            return std::ptr::null_mut();
        }
    }

    unsafe { kmr_suggestion_list_destroy(list_ptr) };
    arr.into_raw()
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeSuggest(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    query: JString,
    limit: jint,
) -> jobjectArray {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "KurmanciEngine handle is closed",
        );
        return std::ptr::null_mut();
    }

    let res = catch_unwind(AssertUnwindSafe(|| {
        let query_str: String = match env.get_string(&query) {
            Ok(s) => s.into(),
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    format!("Invalid query string: {}", e),
                );
                return std::ptr::null_mut();
            }
        };
        let c_query = match CString::new(query_str) {
            Ok(c) => c,
            Err(_) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    "Query contains NUL byte",
                );
                return std::ptr::null_mut();
            }
        };

        let mut list_ptr = std::ptr::null_mut();
        let status = unsafe {
            kmr_engine_suggest(
                handle as *const _,
                c_query.as_ptr(),
                limit as usize,
                &mut list_ptr,
            )
        };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return std::ptr::null_mut();
        }

        convert_suggestion_list(&mut env, list_ptr)
    }));

    match res {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeComplete(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    prefix: JString,
    limit: jint,
) -> jobjectArray {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "KurmanciEngine handle is closed",
        );
        return std::ptr::null_mut();
    }

    let res = catch_unwind(AssertUnwindSafe(|| {
        let prefix_str: String = match env.get_string(&prefix) {
            Ok(s) => s.into(),
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    format!("Invalid prefix string: {}", e),
                );
                return std::ptr::null_mut();
            }
        };
        let c_prefix = match CString::new(prefix_str) {
            Ok(c) => c,
            Err(_) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    "Prefix contains NUL byte",
                );
                return std::ptr::null_mut();
            }
        };

        let mut list_ptr = std::ptr::null_mut();
        let status = unsafe {
            kmr_engine_complete(
                handle as *const _,
                c_prefix.as_ptr(),
                limit as usize,
                &mut list_ptr,
            )
        };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return std::ptr::null_mut();
        }

        convert_suggestion_list(&mut env, list_ptr)
    }));

    match res {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativeCorrect(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    input: JString,
    limit: jint,
) -> jobjectArray {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "KurmanciEngine handle is closed",
        );
        return std::ptr::null_mut();
    }

    let res = catch_unwind(AssertUnwindSafe(|| {
        let input_str: String = match env.get_string(&input) {
            Ok(s) => s.into(),
            Err(e) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    format!("Invalid input string: {}", e),
                );
                return std::ptr::null_mut();
            }
        };
        let c_input = match CString::new(input_str) {
            Ok(c) => c,
            Err(_) => {
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$InvalidArgumentException",
                    "Input contains NUL byte",
                );
                return std::ptr::null_mut();
            }
        };

        let mut list_ptr = std::ptr::null_mut();
        let status = unsafe {
            kmr_engine_correct(
                handle as *const _,
                c_input.as_ptr(),
                limit as usize,
                &mut list_ptr,
            )
        };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return std::ptr::null_mut();
        }

        convert_suggestion_list(&mut env, list_ptr)
    }));

    match res {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "system" fn Java_org_kurmanci_NativeModule_nativePredict(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    context_words: jobjectArray,
    limit: jint,
) -> jobjectArray {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "KurmanciEngine handle is closed",
        );
        return std::ptr::null_mut();
    }

    let res = catch_unwind(AssertUnwindSafe(|| {
        let count = match env
            .get_array_length(unsafe { &jni::objects::JObjectArray::from_raw(context_words) })
        {
            Ok(n) => n as usize,
            Err(_) => 0,
        };

        let mut c_strings = Vec::with_capacity(count);
        let mut ptrs = Vec::with_capacity(count);

        let context_array = unsafe { jni::objects::JObjectArray::from_raw(context_words) };
        for i in 0..count {
            let obj = match env.get_object_array_element(&context_array, i as jint) {
                Ok(o) => o,
                Err(_) => return std::ptr::null_mut(),
            };
            let j_str: JString = obj.into();
            let string_val: String = match env.get_string(&j_str) {
                Ok(s) => s.into(),
                Err(_) => return std::ptr::null_mut(),
            };
            let c_str = match CString::new(string_val) {
                Ok(c) => c,
                Err(_) => return std::ptr::null_mut(),
            };
            ptrs.push(c_str.as_ptr());
            c_strings.push(c_str);
        }

        let mut list_ptr = std::ptr::null_mut();
        let status = unsafe {
            kmr_engine_predict_next(
                handle as *const _,
                if count > 0 {
                    ptrs.as_ptr()
                } else {
                    std::ptr::null()
                },
                count,
                limit as usize,
                &mut list_ptr,
            )
        };

        if status != KMR_OK {
            throw_kmr_exception(&mut env, status);
            return std::ptr::null_mut();
        }

        if list_ptr.is_null() {
            return std::ptr::null_mut();
        }

        let mut len: usize = 0;
        let st = unsafe { kmr_prediction_list_len(list_ptr, &mut len) };
        if st != KMR_OK {
            unsafe { kmr_prediction_list_destroy(list_ptr) };
            throw_kmr_exception(&mut env, st);
            return std::ptr::null_mut();
        }

        let candidate_class = match env.find_class("org/kurmanci/PredictionCandidate") {
            Ok(c) => c,
            Err(e) => {
                unsafe { kmr_prediction_list_destroy(list_ptr) };
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$NativeException",
                    format!("Class org.kurmanci.PredictionCandidate not found: {}", e),
                );
                return std::ptr::null_mut();
            }
        };

        let arr = match env.new_object_array(len as jint, &candidate_class, JObject::null()) {
            Ok(a) => a,
            Err(e) => {
                unsafe { kmr_prediction_list_destroy(list_ptr) };
                let _ = env.throw_new(
                    "org/kurmanci/KurmanciException$NativeException",
                    format!("Failed to allocate PredictionCandidate array: {}", e),
                );
                return std::ptr::null_mut();
            }
        };

        for i in 0..len {
            let mut item: kmr_prediction_item = unsafe { std::mem::zeroed() };
            let get_st = unsafe { kmr_prediction_list_get(list_ptr, i, &mut item) };
            if get_st != KMR_OK {
                unsafe { kmr_prediction_list_destroy(list_ptr) };
                throw_kmr_exception(&mut env, get_st);
                return std::ptr::null_mut();
            }

            let text_str = unsafe {
                if item.text.is_null() {
                    ""
                } else {
                    CStr::from_ptr(item.text).to_str().unwrap_or("")
                }
            };
            let j_text = match env.new_string(text_str) {
                Ok(s) => s,
                Err(_) => {
                    unsafe { kmr_prediction_list_destroy(list_ptr) };
                    return std::ptr::null_mut();
                }
            };

            let obj = match env.new_object(
                &candidate_class,
                "(Ljava/lang/String;JII)V",
                &[
                    (&j_text).into(),
                    (item.count as jlong).into(),
                    (item.probability_millionths as jint).into(),
                    (item.source as jint).into(),
                ],
            ) {
                Ok(o) => o,
                Err(_) => {
                    unsafe { kmr_prediction_list_destroy(list_ptr) };
                    return std::ptr::null_mut();
                }
            };

            if env.set_object_array_element(&arr, i as jint, &obj).is_err() {
                unsafe { kmr_prediction_list_destroy(list_ptr) };
                return std::ptr::null_mut();
            }
        }

        unsafe { kmr_prediction_list_destroy(list_ptr) };
        arr.into_raw()
    }));

    match res {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}
