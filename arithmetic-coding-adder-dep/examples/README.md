# Examples

This crate has a number of examples, for various types of arithmetic encoding.

## [Integer](./integer.rs)

A simple example showing encoding of integers.

## [Symbolic](./symbolic.rs)

An example using custom symbols (you're not limited to primitive types!).

## [Fixed Length](./fixed_length.rs)

An example that uses a fixed length of symbols, rather than encoding EOF. Uses the `fixed_length` helpers from this crate.

## [Sherlock](./sherlock.rs)

Encodes the entire text of "The Adventures of Sherlock Holmes". By allowing a subset 'alphabet' of all possible characters, greater compression is achieved.

## [Fenwick Tree (Adaptive)](./fenwick_adaptive.rs)

Encodes "The Adventures of Sherlock Holmes" using an adaptive model based on [fenwick trees](https://en.wikipedia.org/wiki/Fenwick_tree).

## [Fenwick Tree (Context-Switcing)](./fenwick_context_switching.rs)

Encodes "The Adventures of Sherlock Holmes" using a *context switching* adaptive model based on [fenwick trees](https://en.wikipedia.org/wiki/Fenwick_tree). Achieves very high compression.
