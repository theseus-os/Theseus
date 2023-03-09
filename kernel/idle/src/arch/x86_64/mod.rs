mod intel;

/// A CPU idle state.
#[derive(Clone, Copy, Debug)]
pub struct IdleState {
    /// The name of the idle state.
    pub name: &'static str,
    /// The value of EAX when calling MWAIT to enter the idle state.
    pub eax: usize,
    /// Whether entering the state flushes the TLB.
    pub tlb_flushed: bool,
    /// The amount of time it takes for the CPU to exit the idle state in
    /// microseconds.
    pub exit_latency: usize,
    /// The amount of time the CPU must spend in the idle state to justify
    /// entering the idle state in microseconds.
    ///
    /// For C1, this is equivalent to the exit latency. For other idle states,
    /// it is roughly three times the exit latency.
    pub target_residency: usize,
}

pub fn idle_states() -> Option<&'static [crate::IdleState]> {
    Some(intel::Model::current()?.idle_states())
}
