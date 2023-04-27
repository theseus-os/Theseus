Macro which helps writing cross-platform interrupt handlers.

# Arguments

- `$name`: the name of the function
- `$x86_64_eoi_param`: `Some(irq_num)` if this interrupt can be handled while
  the PIC chip is active and the handler returns `HandlerDidNotSendEoi`; `None` otherwise.
  Ignored on `aarch64`. See [`eoi`] for more information. If the IRQ number isn't
  constant and this interrupt can happen with the PIC chip active, call [`eoi`]
  manually as in Example 2.
- `$stack_frame`: Name for the [`InterruptStackFrame`] parameter.
- `$code`: The code for the interrupt handler itself. It must return an [`crate::EoiBehaviour`] enum.

# Example 1

This simply logs the stack frame to the console.

```ignore
interrupt_handler!(my_int_0x29_handler, Some(interrupts::IRQ_BASE_OFFSET + 0x9), stack_frame, {
    trace!("my_int_0x29_handler running! stack frame: {:?}", stack_frame);

    // loop {}

    EoiBehaviour::HandlerDidNotSendEoi
});
```

# Example 2

Here's how [`eoi`] can be called manually. Note how we can pass `None` as
`$x86_64_eoi_param`, since we call [`eoi`] in the handler.

```ignore
interrupt_handler!(my_int_0x29_handler, None, stack_frame, {
    trace!("my_int_0x29_handler running! stack frame: {:?}", stack_frame);

    #[cfg(target_arch = "x86_64")]
    let irq_num = 0x29;

    #[cfg(target_arch = "aarch64")]
    let irq_num = 0x29;

    // Calling `eoi` manually:
    {
        #[cfg(target_arch = "x86_64")]
        eoi(Some(irq_num));

        #[cfg(target_arch = "aarch64")]
        eoi(irq_num);
    }

    EoiBehaviour::HandlerSentEoi
});
```
