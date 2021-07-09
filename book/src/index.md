# Introduction to Theseus

*Note: for general info about Theseus and a quick start guide, see the [top-level README](https://github.com/theseus-os/Theseus#readme).*


Theseus is a new OS written from scratch in [Rust](https://www.rust-lang.org/) to experiment with novel OS structure, better state management, and how to leverage **intralingual design** principles to shift OS responsibilities like resource management into the compiler.

Continue to the next chapter to learn more about Theseus, or feel free to check out our [published academic papers](misc/papers_presentations.md) for a deep dive into the research and design concept behind Theseus.


### What's in a name? 

> The ship wherein Theseus and the youth of Athens returned from Crete had thirty oars, and was preserved by the Athenians down even to the time of Demetrius Phalereus, for they took away the old planks as they decayed, putting in new and stronger timber in their places, in so much that this ship became a standing example among the philosophers, for the logical question of things that grow; one side holding that the ship remained the same, and the other contending that it was not the same.
> &nbsp; — &nbsp; *Plutarch, Theseus*

The name "Theseus" was inspired by *The Ship of Theseus*, an ancient Greek metaphysical paradox and thought experiment that pondered: "if you iteratively replace every individual piece of an object, is that re-built object still the same object?"

Though we do not attempt to answer this question, we do wish to enable any and every OS component to be replaced,  across all layers of the system, at runtime without rebooting. This goal of easy and arbitrary *live evolution* was (and still is) one of the original motivating factors behind Theseus.
