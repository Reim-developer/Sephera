# Sephera

**Know what breaks before you touch it.**

Sephera builds a real dependency graph from your actual imports and answers: what breaks if I change this file?

Docs: https://sephera.vercel.app

## Build & run

```sh
cargo install --git https://github.com/Reim-developer/Sephera
sephera impact <file>
```

Or build from source:

```sh
git clone https://github.com/Reim-developer/Sephera
cd Sephera
cargo build --release
./target/release/sephera impact <file>
```

Prebuilt binaries: https://github.com/Reim-developer/Sephera/releases

## License

GPLv3 - see [LICENSE](LICENSE).
