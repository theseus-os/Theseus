use crate::Duration;
use apic::{
    has_x2apic, InterruptChip, LocalApic, APIC_DISABLE, APIC_TIMER_PERIODIC, IA32_X2APIC_CUR_COUNT,
    IA32_X2APIC_DIV_CONF, IA32_X2APIC_ESR, IA32_X2APIC_INIT_COUNT, IA32_X2APIC_LVT_THERMAL,
    IA32_X2APIC_LVT_TIMER,
};
use kernel_config::time::CONFIG_TIMESLICE_PERIOD_MICROSECONDS;
use lockable::Lockable;
use log::{error, info, trace};

pub(crate) fn exists() -> bool {
    // TODO: Is this right?
    match apic::INTERRUPT_CHIP.load() {
        InterruptChip::APIC | InterruptChip::X2APIC => true,
        InterruptChip::PIC => false,
    }
}

pub(crate) fn init() -> Result<(), &'static str> {
    let is_x2 = match apic::INTERRUPT_CHIP.load() {
        InterruptChip::APIC => false,
        InterruptChip::X2APIC => true,
        InterruptChip::PIC => return Err("system is using PIC"),
    };

    let mut apic = apic::get_my_apic()
        .ok_or("couldn't get LAPIC handle")?
        .lock_mut();
    
    if is_x2 {
        init_timer_x2apic(&mut apic);
        Ok(())
    } else {
        init_timer(&mut apic)
    }
}

/// Returns the number of APIC ticks that occurred during the given `duration`.
fn calibrate_apic_timer(apic: &mut LocalApic, duration: Duration) -> Result<u32, &'static str> {
    assert!(!has_x2apic(), "an x2apic system must not use calibrate_apic_timer(), it should use calibrate_x2apic_timer_() instead.");

    if let Some(ref mut regs) = apic.regs {
        regs.timer_divide.write(3); // set divide value to 16
        const INITIAL_COUNT: u32 = 0xFFFF_FFFF; // the max count, since we're counting down

        regs.timer_initial_count.write(INITIAL_COUNT); // set counter to max value

        // wait or the given period using the PIT clock
        crate::pit::wait(duration).unwrap();

        regs.lvt_timer.write(APIC_DISABLE); // stop apic timer
        let after = regs.timer_current_count.read();
        let elapsed = INITIAL_COUNT - after;
        Ok(elapsed)
    } else {
        error!("calibrate_apic_timer(): FATAL ERROR: regs (ApicRegisters) were None! Were they initialized right?");
        Err("calibrate_apic_timer(): FATAL ERROR: regs (ApicRegisters) were None! Were they initialized right?")
    }
}

/// Returns the number of APIC ticks that occurred during the given `duration`.
fn calibrate_x2apic_timer(duration: Duration) -> u64 {
    assert!(has_x2apic(), "an apic/xapic system must not use calibrate_x2apic_timer(), it should use calibrate_apic_timer() instead.");
    unsafe {
        wrmsr(IA32_X2APIC_DIV_CONF, 3);
    } // set divide value to 16
    const INITIAL_COUNT: u64 = 0xFFFF_FFFF;

    unsafe {
        wrmsr(IA32_X2APIC_INIT_COUNT, INITIAL_COUNT);
    } // set counter to max value

    // wait or the given period using the PIT clock
    crate::pit::wait(duration).unwrap();

    unsafe {
        wrmsr(IA32_X2APIC_LVT_TIMER, APIC_DISABLE as u64);
    } // stop apic timer
    let after = rdmsr(IA32_X2APIC_CUR_COUNT);
    let elapsed = INITIAL_COUNT - after;
    elapsed
}

fn init_timer(apic: &mut LocalApic) -> Result<(), &'static str> {
    assert!(
        !has_x2apic(),
        "an x2apic system must not use init_timer(), it should use init_timer_x2apic() instead."
    );
    let apic_period = if cfg!(apic_timer_fixed) {
        info!(
            "apic_timer_fixed config: overriding APIC timer period to {}",
            0x10000
        );
        0x10000 // for bochs, which doesn't do apic periods right
    } else {
        calibrate_apic_timer(apic, Duration::from_micros(CONFIG_TIMESLICE_PERIOD_MICROSECONDS.into()))?
    };
    trace!(
        "APIC {}, timer period count: {}({:#X})",
        apic.apic_id,
        apic_period,
        apic_period
    );

    if let Some(ref mut regs) = apic.regs {
        regs.timer_divide.write(3); // set divide value to 16 ( ... how does 3 => 16 )
                                    // map APIC timer to an interrupt handler in the IDT
        regs.lvt_timer.write(0x22 | APIC_TIMER_PERIODIC);
        regs.timer_initial_count.write(apic_period);

        regs.lvt_thermal.write(0);
        regs.lvt_error.write(0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        regs.timer_divide.write(3);

        Ok(())
    } else {
        error!("calibrate_apic_timer(): FATAL ERROR: regs (ApicRegisters) were None! Were they initialized right?");
        Err("calibrate_apic_timer(): FATAL ERROR: regs (ApicRegisters) were None! Were they initialized right?")
    }
}

fn init_timer_x2apic(apic: &mut LocalApic) {
    assert!(
        has_x2apic(),
        "an apic/xapic system must not use init_timerx2(), it should use init_timer() instead."
    );
    let x2apic_period = if cfg!(apic_timer_fixed) {
        info!(
            "apic_timer_fixed config: overriding X2APIC timer period to {}",
            0x10000
        );
        0x10000 // for bochs, which doesn't do x2apic periods right
    } else {
        calibrate_x2apic_timer(Duration::from_micros(CONFIG_TIMESLICE_PERIOD_MICROSECONDS.into()))
    };
    trace!(
        "X2APIC {}, timer period count: {}({:#X})",
        apic.apic_id,
        x2apic_period,
        x2apic_period
    );

    unsafe {
        wrmsr(IA32_X2APIC_DIV_CONF, 3); // set divide value to 16 ( ... how does 3 => 16 )

        // map X2APIC timer to an interrupt handler in the IDT, which we currently use IRQ 0x22 for
        wrmsr(IA32_X2APIC_LVT_TIMER, 0x22 | APIC_TIMER_PERIODIC as u64);
        wrmsr(IA32_X2APIC_INIT_COUNT, x2apic_period);

        wrmsr(IA32_X2APIC_LVT_THERMAL, 0);
        wrmsr(IA32_X2APIC_ESR, 0);

        // os dev wiki guys say that setting this again as a last step helps on some strange hardware.
        wrmsr(IA32_X2APIC_DIV_CONF, 3);
    }
}

// Below: temporary functions for reading MSRs that aren't yet in the `x86_64` crate.

fn rdmsr(msr: u32) -> u64 {
    unsafe { x86_64::registers::model_specific::Msr::new(msr).read() }
}

unsafe fn wrmsr(msr: u32, value: u64) {
    x86_64::registers::model_specific::Msr::new(msr).write(value)
}

// pub(crate) fn now() -> Duration {}
