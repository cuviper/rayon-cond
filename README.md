# rayon-cond

Experimental iterator wrapper that is conditionally parallel or serial, using
Rayon's `ParallelIterator` or the standard `Iterator` respectively.

## Usage

First add this crate to your `Cargo.toml`:

```toml
[dependencies]
rayon-cond = "0.1"
```

Then in your code, it may be used something like this:

```rust
extern crate rayon_cond;

use rayon_cond::CondIterator;

fn main() {
    let args: Vec<_> = std::env::args().collect();

    // Run in parallel if there are an even number of args
    let par = args.len() % 2 == 0;

    CondIterator::new(args, par).enumerate().for_each(|(i, arg)| {
        println!("arg {}: {:?}", i, arg);
    });
}
```

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
  https://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or
  https://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
