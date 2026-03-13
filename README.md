# phonk.rs

`no_std`, `no_alloc` library for monophonic pitch detection, based on the *bitstream
autocorrelation* algorithm. The library is designed for embedded environments.

## Usage

```rust
use phonk::phonk;
let mut phonk = phonk!(batch_size, sample_rate, min_freq, max_freq);

// When new audio samples are available,
phonk.push_samples( & [/* audio samples here */]);
let pitch = phonk.run() ?;
```
