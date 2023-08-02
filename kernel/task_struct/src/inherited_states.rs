//! Defines inherited states.

use alloc::sync::Arc;

use environment::Environment;
use memory::MmiRef;
use mod_mgmt::{AppCrateRef, CrateNamespace};
use spin::Mutex;

use crate::Task;

/// The states used to initialize a new `Task` when creating it; see [`Task::new()`].
///
/// Currently, this includes the states given in the [`InheritedStates::Custom`] variant.
pub enum InheritedStates<'t> {
    /// The new `Task` will inherit its states from the enclosed `Task`.
    FromTask(&'t Task),
    /// The new `Task` will be initialized with the enclosed custom states.
    Custom {
        mmi: MmiRef,
        namespace: Arc<CrateNamespace>,
        env: Arc<Mutex<Environment>>,
        app_crate: Option<Arc<AppCrateRef>>,
    }
}

impl<'t> From<&'t Task> for InheritedStates<'t> {
    fn from(task: &'t Task) -> Self {
        Self::FromTask(task)
    }
}

impl<'t> InheritedStates<'t> {
    pub(crate) fn into_tuple(self) -> (
        MmiRef,
        Arc<CrateNamespace>,
        Arc<Mutex<Environment>>,
        Option<Arc<AppCrateRef>>,
    ) {
        match self {
            Self::FromTask(task) => (
                task.mmi.clone(),
                task.namespace.clone(),
                task.inner.lock().env.clone(),
                task.app_crate.clone(),
            ),
            Self::Custom { mmi, namespace, env, app_crate } => (
                mmi,
                namespace,
                env,
                app_crate,
            )
        }
    }
}
