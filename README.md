# Packed File Archive (.pfa)
The Packed File Archive is a simple format for containing multiple files and/or directories inside of a single packed file, with fast random reads, optional LZ4 compression, AES256-GCM encryption, and Reed Solomon BCH error correction.

## Specification
The specification for Packed File Archive format can be found inside of [design_spec.md](design_spec.md)

## Run tests
To run the unit tests, execute `cargo test` in your terminal.
