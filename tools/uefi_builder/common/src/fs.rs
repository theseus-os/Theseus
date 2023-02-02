//! Taken from rust-osdev/bootloader

use anyhow::Context;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Seek},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

/// Create disk images for booting on UEFI systems.
pub struct UefiBoot {
    kernel: PathBuf,
    extra_files: Vec<(PathBuf, PathBuf)>,
}

impl UefiBoot {
    /// Start creating a disk image for the given bootloader ELF executable.
    pub fn new(kernel_path: &Path) -> Self {
        Self {
            kernel: kernel_path.to_owned(),
            extra_files: Vec::new(),
        }
    }

    /// Create a bootable BIOS disk image at the given path.
    pub fn create_disk_image(
        &self,
        bootloader_host_path: &Path,
        bootloader_efi_path: &Path,
        out_path: &Path,
    ) -> anyhow::Result<()> {
        let fat_partition = self
            .create_fat_partition(bootloader_efi_path, bootloader_host_path)
            .context("failed to create FAT partition")?;

        create_gpt_disk(fat_partition.path(), out_path)
            .context("failed to create UEFI GPT disk image")?;

        fat_partition
            .close()
            .context("failed to delete FAT partition after disk image creation")?;

        Ok(())
    }

    /// Adds a file to the disk image.
    pub fn add_file(&mut self, image_path: PathBuf, host_path: PathBuf) {
        self.extra_files.push((image_path, host_path));
    }

    /// Creates an UEFI-bootable FAT partition with the kernel.
    fn create_fat_partition(
        &self,
        bootloader_efi_path: &Path,
        bootloader_host_path: &Path,
    ) -> anyhow::Result<NamedTempFile> {
        let mut files = BTreeMap::new();
        files.insert(bootloader_efi_path, bootloader_host_path);
        files.insert(Path::new("kernel.elf"), self.kernel.as_path());

        for (image_path, host_path) in &self.extra_files {
            files.insert(image_path, host_path);
        }

        let out_file = NamedTempFile::new().context("failed to create temp file")?;
        create_fat_filesystem(files, out_file.path())
            .context("failed to create UEFI FAT filesystem")?;

        Ok(out_file)
    }
}

pub fn create_gpt_disk(fat_image: &Path, out_gpt_path: &Path) -> anyhow::Result<()> {
    // create new file
    let mut disk = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(out_gpt_path)
        .with_context(|| format!("failed to create GPT file at `{}`", out_gpt_path.display()))?;

    // set file size
    let partition_size: u64 = fs::metadata(fat_image)
        .context("failed to read metadata of fat image")?
        .len();
    let disk_size = partition_size + 1024 * 64; // for GPT headers
    disk.set_len(disk_size)
        .context("failed to set GPT image file length")?;

    // create a protective MBR at LBA0 so that disk is not considered
    // unformatted on BIOS systems
    let mbr = gpt::mbr::ProtectiveMBR::with_lb_size(
        u32::try_from((disk_size / 512) - 1).unwrap_or(0xFF_FF_FF_FF),
    );
    mbr.overwrite_lba0(&mut disk)
        .context("failed to write protective MBR")?;

    // create new GPT structure
    let block_size = gpt::disk::LogicalBlockSize::Lb512;
    let mut gpt = gpt::GptConfig::new()
        .writable(true)
        .initialized(false)
        .logical_block_size(block_size)
        .create_from_device(Box::new(&mut disk), None)
        .context("failed to create GPT structure in file")?;
    gpt.update_partitions(Default::default())
        .context("failed to update GPT partitions")?;

    // add new EFI system partition and get its byte offset in the file
    let partition_id = gpt
        .add_partition("boot", partition_size, gpt::partition_types::EFI, 0, None)
        .context("failed to add boot EFI partition")?;
    let partition = gpt
        .partitions()
        .get(&partition_id)
        .context("failed to open boot partition after creation")?;
    let start_offset = partition
        .bytes_start(block_size)
        .context("failed to get start offset of boot partition")?;

    // close the GPT structure and write out changes
    gpt.write().context("failed to write out GPT changes")?;

    // place the FAT filesystem in the newly created partition
    disk.seek(io::SeekFrom::Start(start_offset))
        .context("failed to seek to start offset")?;
    io::copy(
        &mut File::open(fat_image).context("failed to open FAT image")?,
        &mut disk,
    )
    .context("failed to copy FAT image to GPT disk")?;

    Ok(())
}

pub fn create_fat_filesystem(
    files: BTreeMap<&Path, &Path>,
    out_fat_path: &Path,
) -> anyhow::Result<()> {
    const MB: u64 = 1024 * 1024;
    const SECTOR: u64 = 512;

    // calculate needed size
    let mut needed_size = 0;
    for path in files.values() {
        let file_size = fs::metadata(path)
            .with_context(|| format!("failed to read metadata of file `{}`", path.display()))?
            .len();
        // round up to sector size
        let sector_aligned_file_size = ((file_size + SECTOR)/SECTOR) * SECTOR;
        needed_size += sector_aligned_file_size;
    }
    
    // create new filesystem image file at the given path and set its length
    let fat_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(out_fat_path)
        .unwrap();
    // Add an extra 2MB of size, which seems to be sufficient for our needs.
    let fat_size_padded_and_rounded = ((needed_size + (2*MB))/MB) * MB;
    fat_file.set_len(fat_size_padded_and_rounded).unwrap();

    // choose a file system label
    let mut label = *b"Theseus OS ";
    if let Some(path) = files.get(Path::new("kernel.elf")) {
        if let Some(name) = path.file_stem() {
            let converted = name.to_string_lossy();
            let name = converted.as_bytes();
            let mut new_label = [0u8; 11];
            let name = &name[..usize::min(new_label.len(), name.len())];
            let slice = &mut new_label[..name.len()];
            slice.copy_from_slice(name);
            label = new_label;
        }
    }

    // format the file system and open it
    let format_options = fatfs::FormatVolumeOptions::new().volume_label(label);
    fatfs::format_volume(&fat_file, format_options).context("Failed to format FAT file")?;
    let filesystem = fatfs::FileSystem::new(&fat_file, fatfs::FsOptions::new())
        .context("Failed to open FAT file system of UEFI FAT file")?;

    // copy files to file system
    let root_dir = filesystem.root_dir();
    for (target_path, file_path) in files {
        // create parent directories
        let ancestors: Vec<_> = target_path.ancestors().skip(1).collect();
        for ancestor in ancestors.into_iter().rev().skip(1) {
            root_dir
                .create_dir(&ancestor.display().to_string())
                .with_context(|| {
                    format!(
                        "failed to create directory `{}` on FAT filesystem",
                        ancestor.display()
                    )
                })?;
        }

        let mut new_file = root_dir
            .create_file(target_path.to_str().unwrap())
            .with_context(|| format!("failed to create file at `{}`", target_path.display()))?;
        new_file.truncate().unwrap();

        let res = copy_file_data(file_path, &mut new_file);
        if res.is_err() {
            let curr_len = fat_file.metadata().unwrap().len();
            let new_len = curr_len + 2*fs::metadata(file_path).unwrap().len() + MB;
            fat_file.set_len(new_len).unwrap();
            // try again with the resized file.
            copy_file_data(file_path, &mut new_file)?;
        }
    }

    Ok(())
}

fn copy_file_data<T: fatfs::ReadWriteSeek>(file_path: &Path, new_file: &mut fatfs::File<T>) -> anyhow::Result<u64> {
    io::copy(
        &mut fs::File::open(file_path)
            .with_context(|| format!("failed to open `{}` for copying", file_path.display()))?,
        new_file,
    )
    .with_context(|| format!("failed to copy `{}` to FAT filesystem", file_path.display()))
}
