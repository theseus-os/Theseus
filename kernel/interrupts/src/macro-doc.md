Macro which helps writing cross-platform interrupt handlers.

# Arguments

- `$name`: the name of the interrupt handler function.
- `$x86_64_eoi_param`: one of two possible values:
  1. the literal underscore character `_`, used to indicate that this value isn't used
     and the interrupt handler does not care about its value.
     * This is useful for interrupt handlers that are aarch64 specific
       or can only occur on x86_64's newer APIC interrupt chips, which do not require
       specifying a specific IRQ number when sending an end of interrupt (EOI).
  2. a valid [`InterruptNumber`] if this interrupt may be handled by the legacy PIC chip
     on x86_64, which is used if the handler returns `HandlerDidNotSendEoi`.
- `$stack_frame`: Name for the [`InterruptStackFrame`] parameter.
- `$code`: The code for the interrupt handler itself, which must return [`crate::EoiBehaviour`].

## Example 1

This simply logs the stack frame to the console.

```ignore
interrupt_handler!(my_int_0x29_handler, interrupts::IRQ_BASE_OFFSET + 0x9, stack_frame, {
    trace!("my_int_0x29_handler running! stack frame: {:?}", stack_frame);

    EoiBehaviour::HandlerDidNotSendEoi
});
```

## Example 2

Here's how [`eoi`] can be called manually. Note how we use `_` for the second parameter
in the macro (the `$x86_64_eoi_param`), since we call [`eoi`] in the handler.

```ignore
interrupt_handler!(my_int_0x29_handler, _, stack_frame, {
    trace!("my_int_0x29_handler running! stack frame: {:?}", stack_frame);

    // Call `eoi` manually.
    let irq_num = 0x29;
    eoi(irq_num);

    EoiBehaviour::HandlerSentEoi
});
```
