# trusted_chunk
This crate contains a TrustedChunk type with verified methods that prevent creating chunks with overlapping ranges.
This is done by storing information about all previously created chunks in a verified bookkeeping data structure (an array before heap initialization and a linked list after). A TrustedChunkAllocator stores the bookkeeping structures and provides the only public interface to create a TrustedChunk. All chunks created from a single TrustedChunkAllocator instance do not overlap.

## Running Prusti on this crate
1. Download the pre-compiled binary for Release v-2023-01-26 from [here](https://github.com/viperproject/prusti-dev/releases/tag/v-2023-01-26-1935)
2. Navigate to the prusti-release folder
3. Run this command 
```
./prusti-rustc <path to trusted_chunk/src/lib.rs> -Pcheck_overflows=false --crate-type=lib --cfg "prusti" --cfg "std"
```

## Notes for Prusti improvements
1. Eq, PartialEq, Ord, etc. traits should be pure by default
2. Functions for structs with generics lead to internal compiler errors

## Working with cargo-prusti
We can also use the cargo-prusti tool by running it in the repo with the Cargo.toml file, and adding a Prusti.toml file with the prusti flags.
We would also have to change the syntax for conditional compilation in the crate to [cfg(feature = "prusti")]
```
./<path>/cargo-prusti  --features prusti
```