
    // /// Reinterprets this `MappedPages`'s underlying memory region as a struct of the given type,
    // /// i.e., overlays a struct on top of this mapped memory region. 
    // /// 
    // /// # Arguments
    // /// `offset`: the offset into the memory region at which the struct is located (where it should start).
    // /// 
    // /// Returns a reference to the new struct (`&T`) that is formed from the underlying memory region,
    // /// with a lifetime dependent upon the lifetime of this `MappedPages` object.
    // /// This ensures safety by guaranteeing that the returned struct reference 
    // /// cannot be used after this `MappedPages` object is dropped and unmapped.
    // pub fn as_unsized_type<T: Sized, U>(&self, offset: usize) -> Result<&U, &'static str> {
    //     let size = mem::size_of::<T>();
    //     if true {
    //         debug!("MappedPages::as_unsized_type(): requested type {} -> {} with size {} at offset {}, MappedPages size {}!",
    //             core::any::type_name::<T>(),
    //             core::any::type_name::<U>(),
    //             size, offset, self.size_in_bytes()
    //         );
    //     }

    //     // check that size of the type T fits within the size of the mapping
    //     let end = offset + size;
    //     if end > self.size_in_bytes() {
    //         error!("MappedPages::as_type(): requested type {} has size {}, which is too large at offset {} for MappedPages of size {}!",
    //             core::any::type_name::<T>(),
    //             size, offset, self.size_in_bytes()
    //         );
    //         return Err("requested type and offset would not fit within the MappedPages bounds");
    //     }

    //     // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
    //     let t: &T = unsafe { 
    //         mem::transmute(self.pages.start_address() + offset)
    //     };

    //     let u: &U = &*t;
    //     Ok(u)
    // }


    // /// Reinterprets this `MappedPages`'s underlying memory region as a dynamically-sized type, i.e.,
    // /// a tuple composed of an type `T` followed directly by `[S]`, 
    // /// a dynamically-sized slice of `slice_length` elements of type `S`.
    // /// 
    // /// In other words, the returned reference is `(&T, &[S])`.
    // /// 
    // /// The slice will start directly after the `T` type ends, so if the size of `T` is 32 bytes,
    // /// the slice will start at `(offset + 32)` and end at `(offset + 32) + (slice_length * size_of::<S>())`.
    // /// 
    // /// # Arguments
    // /// * `offset`: the offset into the memory region at which the struct is located (where it should start).
    // /// * `slice_length`: the number of elements of type `S` that comprise the end of the struct.
    // /// 
    // /// This is effectively a composition of [`as_type`](#method.as_type) and [`as_slice`](#method.as_slice).
    // /// 
    // /// # Alignment Warning
    // /// Because this function returns a tuple, the type `T` must end on a normal alignment boundary (at least 32-bit aligned).
    // /// Otherwise, the data may be incorrectly represented; however, this is always a problem with reinterpreting 
    // /// MappedPages as any arbitrary type -- that type must be defined properly. 
    // pub fn as_dynamically_sized_type<T: Sized, S: Sized>(&self, offset: usize, slice_length: usize) -> Result<(&T, &[S]), &'static str> {
    //     let type_size = mem::size_of::<T>();
    //     let slice_offset = offset + type_size;
    //     let slice_size = slice_length * mem::size_of::<S>();
    //     let total_size = type_size + slice_size;
    //     if true {
    //         debug!("MappedPages::as_dynamically_sized_type(): total size {}, requested type {} (size {}) and slice [{}; {}] (slice size {}) at offset {}, MappedPages size {}!",
    //             total_size,
    //             core::any::type_name::<T>(),
    //             type_size,
    //             core::any::type_name::<S>(),
    //             slice_length,
    //             slice_size,
    //             offset,
    //             self.size_in_bytes()
    //         );
    //     }

    //     let end = offset + total_size;
    //     if end > self.size_in_bytes() {
    //         error!("MappedPages::as_dynamically_sized_type(): requested type {} (size {}) and slice [{}; {}] (slice size {}) at offset {} is too large for MappedPages size {}, its total size is {}.",
    //             core::any::type_name::<T>(),
    //             type_size,
    //             core::any::type_name::<S>(),
    //             slice_length,
    //             slice_size,
    //             offset,
    //             self.size_in_bytes(),
    //             total_size,
    //         );
    //         return Err("requested type, slice, and offset would not fit within the MappedPages bounds");
    //     }

    //     // SAFE: we guarantee the size and lifetime are within that of this MappedPages object
    //     Ok( unsafe {
    //         (
    //             mem::transmute(self.pages.start_address().value() + offset),
    //             slice::from_raw_parts((self.pages.start_address().value() + slice_offset) as *const S, slice_length),
    //         )
    //     })
    // }

