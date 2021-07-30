# Theseus Ideas and Inspiration

Theseus is a safe-language OS, meaning that it relies on type safety and memory safety guarantees from the Rust language and compiler to enforce protection and isolation between tasks and components. 
As such, it foregoes hardware protection, which generally results in higher efficiency due to the ability to bypass overhead stemming from switching privilege modes and address spaces. 

This is possible only when all applications are written in safe Rust, which prevents them from circumventing any type-based restrictions to cause unintended or undefined behavior.

Check out [this presentation slide deck](https://docs.google.com/presentation/d/e/2PACX-1vSa0gp8sbq8S9MB4V-FYjs6xJGIPm0fsZSVdtZ9U2bQWRX9gngwztXTIJiRwxtAosLWPk0v60abDMTU/pub?start=false&loop=false) to learn more about how we ensure protection and isolation in Theseus based on the foundation of Rust's type and memory safety guarantees.

For more details about Theseus's research merit and novel design principles, see our [selected list of papers and presentations here](../misc/papers_presentations.md).


## P.I.E. Principle
The P.I.E. principle is one of the guiding lights in the design of Theseus and much of our other systems software research.
The main idea is that there are three pillars of computing goals, of which only 2 of 3 can be achieved simultaneously:
<!-- cspell:disable -->
1. **P**erformance
2. **I**solation
3. **E**fficiency
<!-- cspell:enable -->

Traditionally, systems software designers have looked to hardware to provide all three -- high performance, strong isolation, and efficiency (low overhead). 
We believe that hardware cannot fully realize all three:
* Hardware can realize high performance with high efficiency, but only in the absence of hardware-provided isolation (protection).
* Hardware can realize high performance with strong isolation, but is inefficient, e.g., due to switching between privilege modes and address spaces.
* Hardware can realize strong isolation at the cost of lower efficiency, which reduces performance.


We assert that hardware should *only* be responsible for increasing performance, i.e., via various accelerators and dedicated improvements, but should have *no role* (or a minimal role) in providing isolation, safety, and security.
Isolation and efficiency should be the responsibility of software alone.

We sometimes refer to this as the **PHIS** principle: **Performance** in **Hardware**, **Isolation** in **Software**.

*But why?*

For one, speculative execution exploits like Meltdown and Spectre have shown that hardware-ensured isolation does not protect kernel data from untrusted user space applications to the extent we once thought. It is difficult if not impossible to verify the true behavior of closed-source hardware (CPU architectures), so we turn to open-source software instead, where we have the ability to verify the OS, compiler, language libraries, and more. 

In addition, modern languages like Rust are able to ensure type safety and memory safety at compile time, without the overhead of traditional safe/managed languages that rely upon inefficient garbage collection and transparent heap-based object management.
Thus, we can leverage these safety guarantees to ensure that compiled code does not violation isolation between tasks (threads of execution) and software modules without the need for significant runtime checks.


Theseus transcends the reliance on hardware to provide isolation, and completely foregoes hardware privilege levels (x86's Ring 0 vs. Ring 3 distinction) and multiple address spaces.
Instead, we run all code at Ring 0 in a single virtual address space, including user applications that are written in purely safe Rust.
This maximizes efficiency whilst preserving protection, because we can guarantee at compile time that a given application or kernel component cannot violate isolation between modules, rendering hardware privilege levels obsolete.
Theseus still does use virtual memory translation provided by the MMU, but simply for convenience and ease of memory management; it can be very difficult and inefficient to directly handle and allocate physical memory for applications, and also to find large contiguous chunks of physical memory. 


## Going beyond safety

We show that it's possible to leverage safe languages and compilers to go much further than just basic isolation and memory safety. 
For more details, read about Theseus's novel concept of intralingual design (coming soon). <!-- TODO [intralingual design here](intralingual.md). -->
