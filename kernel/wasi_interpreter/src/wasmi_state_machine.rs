//! Module for resolving memory and function imports from WebAssembly module into state machine.
//!
//! Subset of functionality borrowed from tomaka/redshirt:
//! <https://github.com/tomaka/redshirt/blob/4df506f68821353a7fd67bb94c4223df6b683e1b/kernel/core/src/scheduler/vm.rs>
//!

#![allow(clippy::type_complexity)]

use alloc::string::String;
use core::{cell::RefCell, convert::TryFrom as _};

use wasmi::{Module, Signature};

#[derive(Debug)]
pub enum NewErr {
    /// Error in the interpreter.
    Interpreter(wasmi::Error),
    /// If a "memory" symbol is provided, it must be a memory.
    MemoryIsntMemory,
    /// A memory object has both been imported and exported.
    MultipleMemoriesNotSupported,
    /// If a "__indirect_function_table" symbol is provided, it must be a table.
    IndirectTableIsntTable,
}

pub struct ProcessStateMachine {
    /// Original module, with resolved imports.
    pub module: wasmi::ModuleRef,

    /// Memory of the module instantiation.
    ///
    /// Right now we only support one unique `Memory` object per process. This is it.
    /// Contains `None` if the process doesn't export any memory object, which means it doesn't use
    /// any memory.
    pub memory: Option<wasmi::MemoryRef>,

    /// Table of the indirect function calls.
    ///
    /// In WASM, function pointers are in reality indices in a table called
    /// `__indirect_function_table`. This is this table, if it exists.
    pub indirect_table: Option<wasmi::TableRef>,
}

impl ProcessStateMachine {
    /// Creates a new process state machine from the given module.
    ///
    /// The closure is called for each import that the module has. It must assign a number to each
    /// import, or return an error if the import can't be resolved. When the VM calls one of these
    /// functions, this number will be returned back in order for the user to know how to handle
    /// the call.
    ///
    /// A single main thread (whose user data is passed by parameter) is automatically created and
    /// is paused at the start of the "_start" function of the module.
    pub fn new(
        module: &Module,
        mut symbols: impl FnMut(&str, &str, &Signature) -> Result<usize, ()>,
    ) -> Result<Self, NewErr> {
        struct ImportResolve<'a> {
            func: RefCell<&'a mut dyn FnMut(&str, &str, &Signature) -> Result<usize, ()>>,
            memory: RefCell<&'a mut Option<wasmi::MemoryRef>>,
        }

        impl<'a> wasmi::ImportResolver for ImportResolve<'a> {
            fn resolve_func(
                &self,
                module_name: &str,
                field_name: &str,
                signature: &wasmi::Signature,
            ) -> Result<wasmi::FuncRef, wasmi::Error> {
                let closure = &mut **self.func.borrow_mut();
                let index = match closure(module_name, field_name, signature) {
                    Ok(i) => i,
                    Err(_) => {
                        return Err(wasmi::Error::Instantiation(
                            format!("Couldn't resolve `{module_name}`:`{field_name}`")
                        ))
                    }
                };

                Ok(wasmi::FuncInstance::alloc_host(signature.clone(), index))
            }

            fn resolve_global(
                &self,
                _module_name: &str,
                _field_name: &str,
                _global_type: &wasmi::GlobalDescriptor,
            ) -> Result<wasmi::GlobalRef, wasmi::Error> {
                Err(wasmi::Error::Instantiation(String::from(
                    "Importing globals is not supported yet",
                )))
            }

            fn resolve_memory(
                &self,
                _module_name: &str,
                _field_name: &str,
                memory_type: &wasmi::MemoryDescriptor,
            ) -> Result<wasmi::MemoryRef, wasmi::Error> {
                let mut mem = self.memory.borrow_mut();
                if mem.is_some() {
                    return Err(wasmi::Error::Instantiation(String::from(
                        "Only one memory object is supported yet",
                    )));
                }

                let new_mem = wasmi::MemoryInstance::alloc(
                    wasmi::memory_units::Pages(usize::try_from(memory_type.initial()).unwrap()),
                    memory_type
                        .maximum()
                        .map(|p| wasmi::memory_units::Pages(usize::try_from(p).unwrap())),
                )
                .unwrap();
                **mem = Some(new_mem.clone());
                Ok(new_mem)
            }

            fn resolve_table(
                &self,
                _module_name: &str,
                _field_name: &str,
                _table_type: &wasmi::TableDescriptor,
            ) -> Result<wasmi::TableRef, wasmi::Error> {
                Err(wasmi::Error::Instantiation(String::from(
                    "Importing tables is not supported yet",
                )))
            }
        }

        let (not_started, imported_memory) = {
            let mut imported_memory = None;
            let resolve = ImportResolve {
                func: RefCell::new(&mut symbols),
                memory: RefCell::new(&mut imported_memory),
            };
            let not_started =
                wasmi::ModuleInstance::new(module, &resolve).map_err(NewErr::Interpreter)?;
            (not_started, imported_memory)
        };

        // TODO: WASM has a special "start" instruction that can be used to designate a function
        // that must be executed before the module is considered initialized. It is unclear whether
        // this is intended to be a function that for example initializes global variables, or if
        // this is an equivalent of "_start". In practice, Rust never seems to generate such as
        // "start" instruction, so for now we ignore it. The code below panics if there is such
        // a "start" item, so we will fortunately not blindly run into troubles.
        let module = not_started.assert_no_start();

        let memory = if let Some(imported_mem) = imported_memory {
            if module
                .export_by_name("memory")
                .map_or(false, |m| m.as_memory().is_some())
            {
                return Err(NewErr::MultipleMemoriesNotSupported);
            }
            Some(imported_mem)
        } else if let Some(mem) = module.export_by_name("memory") {
            if let Some(mem) = mem.as_memory() {
                Some(mem.clone())
            } else {
                return Err(NewErr::MemoryIsntMemory);
            }
        } else {
            None
        };

        let indirect_table = if let Some(tbl) = module.export_by_name("__indirect_function_table") {
            if let Some(tbl) = tbl.as_table() {
                Some(tbl.clone())
            } else {
                return Err(NewErr::IndirectTableIsntTable);
            }
        } else {
            None
        };

        let state_machine = ProcessStateMachine {
            module,
            memory,
            indirect_table,
        };

        Ok(state_machine)
    }
}
