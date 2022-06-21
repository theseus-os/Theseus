//! Routines for parsing the `nano_core`, the fully-linked, already-loaded base kernel image,
//! in other words, the code that is currently executing.
//! As such, it performs no loading, but rather just creates metadata that represents
//! the existing kernel code that was loaded by the bootloader, and adds those functions to the system map.

use crate::serde::SerializedCrate;

use super::CrateNamespace;
use alloc::{collections::BTreeMap, string::String, sync::Arc};
use crate_metadata::StrongCrateRef;
use memory::MappedPages;
use path::Path;
use spin::Mutex;

/// The file name (without extension) that we expect to see in the namespace's kernel crate directory.
/// The trailing period '.' is there to avoid matching the "nano_core-<hash>.o" object file.
const NANO_CORE_FILENAME_PREFIX: &str = "nano_core.";

/// Just like Rust's `try!()` macro, but packages up the given error message in a tuple
/// with the array of 3 MappedPages that must also be returned.
macro_rules! try_mp {
    ($expr:expr, $tp:expr, $rp:expr, $dp:expr) => {
        match $expr {
            Ok(val) => val,
            Err(err_msg) => return Err((err_msg, [$tp, $rp, $dp])),
        }
    };
}

/// Deserializes the file containing the [`SerializedCrate`] representation of the already loaded
/// (and currently running) `nano_core` code.
///
/// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
/// We consider both `GLOBAL` and `WEAK` symbols to be global public symbols; this is necessary because symbols that are
/// compiler builtins, such as memset, memcpy, etc, are symbols with weak linkage in newer versions of Rust (2021 and later).
///
/// # Return
/// If successful, this returns a tuple of the following:
/// * `nano_core_crate_ref`: A reference to the newly-created nano_core crate.
/// * `init_symbols`: a map of symbol name to its constant value, which contains assembler and linker constances.
/// * The number of new symbols added to the symbol map (a `usize`).
///
/// If an error occurs, the returned `Result::Err` contains the passed-in `text_pages`, `rodata_pages`, and `data_pages`
/// because those cannot be dropped, as they hold the currently-running code, and dropping them would cause endless exceptions.
pub fn deserialize_nano_core(
    namespace: &Arc<CrateNamespace>,
    text_pages: MappedPages,
    rodata_pages: MappedPages,
    data_pages: MappedPages,
    verbose_log: bool,
) -> Result<
    (StrongCrateRef, BTreeMap<String, usize>, usize),
    (&'static str, [Arc<Mutex<MappedPages>>; 3]),
> {
    let text_pages = Arc::new(Mutex::new(text_pages));
    let rodata_pages = Arc::new(Mutex::new(rodata_pages));
    let data_pages = Arc::new(Mutex::new(data_pages));

    let (nano_core_file, real_namespace) = try_mp!(
        CrateNamespace::get_crate_object_file_starting_with(namespace, NANO_CORE_FILENAME_PREFIX)
            .ok_or("couldn't find the expected \"nano_core\" kernel file"),
        text_pages,
        rodata_pages,
        data_pages
    );
    let nano_core_file_path = Path::new(nano_core_file.lock().get_absolute_path());
    debug!(
        "deserialize_nano_core(): trying to load and parse the nano_core file: {:?}",
        nano_core_file_path
    );

    let nano_core_file_locked = nano_core_file.lock();
    let size = nano_core_file_locked.len();
    let mapped_pages = try_mp!(
        nano_core_file_locked.as_mapping(),
        text_pages,
        rodata_pages,
        data_pages
    );

    debug!("Parsing nano_core symbol file: size {:#x}({}), mapped_pages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
        size, size, mapped_pages, text_pages, rodata_pages, data_pages);

    let bytes: &[u8] = try_mp!(
        mapped_pages.as_slice(0, size),
        text_pages,
        rodata_pages,
        data_pages
    );

    let (deserialized, _): (SerializedCrate, _) = try_mp!(
        bincode::serde::decode_from_slice(bytes, bincode::config::standard()).map_err(|e| {
            error!("deserialize_nano_core(): error deserializing nano_core: {e}");
            "deserialize_nano_core(): error deserializing nano_core"
        }),
        text_pages,
        rodata_pages,
        data_pages
    );
    drop(nano_core_file_locked);

    Ok(try_mp!(
        deserialized.load(
            nano_core_file,
            real_namespace,
            &text_pages,
            &rodata_pages,
            &data_pages,
            verbose_log,
        ),
        text_pages,
        rodata_pages,
        data_pages
    ))
}
