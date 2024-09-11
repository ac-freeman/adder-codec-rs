// // From https://github.com/danieleades/arithmetic-coding. Only temporary, for initial testing.
// #![allow(missing_docs, unused)]
// //! simple adaptive model using a fenwick tree
//
// use arithmetic_coding::Model;
//
// use super::Weights;
// use crate::codec::compressed::fenwick::ValueError;
//
// #[derive(Debug, Clone)]
// pub struct FenwickModel {
//     weights: Weights,
//     max_denominator: u64,
//     panic_on_saturation: bool,
// }
//
// #[must_use]
// pub struct Builder {
//     model: FenwickModel,
// }
//
// impl Builder {
//     fn new(n_symbols: usize, max_denominator: u64) -> Self {
//         let weights = Weights::new(n_symbols);
//         let model = FenwickModel {
//             weights,
//             max_denominator,
//             panic_on_saturation: false,
//         };
//         Self { model }
//     }
//
//     pub fn panic_on_saturation(mut self) -> Self {
//         self.model.panic_on_saturation = true;
//         self
//     }
//
//     #[must_use]
//     pub fn build(self) -> FenwickModel {
//         self.model
//     }
// }
//
// impl FenwickModel {
//     pub fn builder(n_symbols: usize, max_denominator: u64) -> Builder {
//         Builder::new(n_symbols, max_denominator)
//     }
// }
//
// impl Model for FenwickModel {
//     type B = u64;
//     type Symbol = Input;
//     type ValueError = ValueError;
//
//     fn probability(
//         &self,
//         symbol: Option<&Self::Symbol>,
//     ) -> Result<std::ops::Range<Self::B>, Self::ValueError> {
//         if let Some(s) = symbol.copied() {
//             if s >= self.weights.len() {
//                 Err(ValueError(s))
//             } else {
//                 Ok(self.weights.range(Some(s)))
//             }
//         } else {
//             Ok(self.weights.range(None))
//         }
//     }
//
//     fn max_denominator(&self) -> Self::B {
//         self.max_denominator
//     }
//
//     fn symbol(&self, value: Self::B) -> Option<Self::Symbol> {
//         self.weights.symbol(value)
//     }
//
//     fn denominator(&self) -> Self::B {
//         self.weights.total()
//     }
//
//     fn update(&mut self, symbol: Option<&Self::Symbol>) {
//         if self.panic_on_saturation {
//             debug_assert!(
//                 self.denominator() < self.max_denominator,
//                 "hit max denominator!"
//             );
//         }
//         if self.denominator() < self.max_denominator {
//             self.weights.update(symbol.copied(), 1);
//         }
//     }
// }
