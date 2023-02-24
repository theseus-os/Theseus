use crate::IdleState;
use raw_cpuid::CpuId;

pub(crate) enum Model {
    Broadwell,
}

impl Model {
    pub(crate) fn current() -> Option<Self> {
        let feature_info = CpuId::new().get_feature_info()?;

        match feature_info.model_id() {
            0x3d => Some(Self::Broadwell),
            _ => None,
        }
    }

    pub(crate) fn idle_states(&self) -> &'static [IdleState] {
        match self {
            Self::Broadwell => &[
                IdleState {
                    name: "C1",
                    eax: 0x0,
                    tlb_flushed: false,
                    exit_latency: 2,
                    target_residency: 2,
                },
                IdleState {
                    name: "C1E",
                    eax: 0x1,
                    tlb_flushed: false,
                    exit_latency: 10,
                    target_residency: 20,
                },
                IdleState {
                    name: "C3",
                    eax: 0x10,
                    tlb_flushed: true,
                    exit_latency: 40,
                    target_residency: 100,
                },
                IdleState {
                    name: "C6",
                    eax: 0x20,
                    tlb_flushed: true,
                    exit_latency: 133,
                    target_residency: 400,
                },
                IdleState {
                    name: "C7s",
                    eax: 0x32,
                    tlb_flushed: true,
                    exit_latency: 166,
                    target_residency: 500,
                },
                IdleState {
                    name: "C8",
                    eax: 0x40,
                    tlb_flushed: true,
                    exit_latency: 300,
                    target_residency: 900,
                },
                IdleState {
                    name: "C9",
                    eax: 0x50,
                    tlb_flushed: true,
                    exit_latency: 600,
                    target_residency: 1800,
                },
                IdleState {
                    name: "C10",
                    eax: 0x60,
                    tlb_flushed: true,
                    exit_latency: 2600,
                    target_residency: 7700,
                },
            ],
        }
    }
}
