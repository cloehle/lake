// Copyright 2016 Jeffrey Burdges.

//! Sphinx header layout routines
//!
//! ...


use super::curve::{AlphaBytes,ALPHA_LENGTH};
use super::stream::{Gamma,GammaBytes,GAMMA_LENGTH,SphinxHop};
// use super::node::NodeToken;
use super::error::*;


/// Alias for indexes into a Sphinx header
pub type Length = usize;

/// Sphinx `'static` runtime paramaters 
///
/// We require a `&'static SphinxParams` when used because the
/// protocol specification should be compiled into the binary.
#[derive(Debug)] // Clone, Copy
pub struct SphinxParams {
    /// Unique version identifier for the protocol
    pub protocol_name: &'static str,

    /// Length of the routing information block `Beta`.
    ///
    /// A multiple of the ChaCha blocksize of 64 may produce better performance.
    pub beta_length: Length,

    /// Maximal amount of routing infomrmation in `Beta` consued
    /// by a single sub-hop.
    ///
    /// A multiple of the ChaCha blocksize of 64 may produce better performance.
    pub max_beta_tail_length: Length,

    /// Length of the SURB log.
    ///
    /// A multiple of the ChaCha blocksize of 64 may produce better performance.
    pub surb_log_length: Length,

    /// Approved message body lengths
    pub body_lengths: &'static [Length],
}

/// Returns an initial segment of a `mut &mut [T]` replacing the inner
/// `&mut [T]` with the remainder.  In effect, this executes the command
/// `(return,heap) = heap.split_at_mut(len)` without annoying the borrow
/// checker.  See http://stackoverflow.com/a/42162816/667457
fn reserve<'heap, T>(heap: &mut &'heap mut [T], len: usize) -> &'heap mut [T] {
    let tmp: &'heap mut [T] = ::std::mem::replace(&mut *heap, &mut []);
    let (reserved, tmp) = tmp.split_at_mut(len);
    *heap = tmp;
    reserved
}

/// A version of `reserve` for fixed length arrays.
macro_rules! reserve_fixed { ($heap:expr, $len:expr) => {
    array_mut_ref![reserve($heap,$len),0,$len]
} }

impl SphinxParams {
    /// Sphinx SURB length
    ///
    /// Alpha and Gamma do not appear here currently because we encode
    /// them into the "bottom" of beta; however, this could be changed.
    #[inline(always)]
    pub fn surb_length(&self) -> usize {
         self.beta_length
    }

    /// Sphinx header length
    #[inline(always)]
    pub fn header_length(&self) -> usize {
        ALPHA_LENGTH + GAMMA_LENGTH + self.beta_length
        + self.surb_log_length
        + self.surb_length()
    }

    /// Create a `Box<[u8]>` with the required header length
    /// and containing zeros.
    pub fn boxed_zeroed_header(&self) -> Box<[u8]> {
        let mut v = Vec::with_capacity(self.header_length());
        for _ in 0..self.header_length() { v.push(0); }
        v.into_boxed_slice()
    }

    /// Borrow a mutable slice `&mut [u8]` as a `HeaderRefs` consisting.
    /// of subspices for the various header components.  You may mutate
    /// these freely so that after the borrow ends the original slice
    /// contains the new header. 
    pub fn slice_header<'a>(&'static self, mut header: &'a mut [u8])
      -> SphinxResult<HeaderRefs<'a>>
    {
        if header.len() < self.header_length() {
            return Err( SphinxError::InternalError("Header is too short!") );
        }
        let hr = HeaderRefs {
            params: self,
            alpha: reserve_fixed!(&mut header,ALPHA_LENGTH),
            gamma: reserve_fixed!(&mut header,GAMMA_LENGTH),
            beta: reserve(&mut header,self.beta_length),
            surb_log: reserve(&mut header,self.surb_log_length),
            surb: reserve(&mut header,self.surb_length()),
        };
        if header.len() > 0 {
            return Err( SphinxError::InternalError("Header is too long!") );
        }
        Ok(hr)
    }

    /// Returns an error if the body length is not approved by the paramaters.
    pub fn check_body_length(&self, body_length: Length) -> SphinxResult<()> {
        if self.body_lengths.len() == 0 {
            if body_length == 0 {
                Ok(())  // All body lengths are zero if no body lengths were specified
            } else {
                Err( SphinxError::InternalError("Nonempty body with no body lengths specified.") )
            }
        } else if self.body_lengths.contains(&body_length) {
            Ok(())
        } else {
            Err( SphinxError::BadBodyLength(body_length) )
        }
    }
}

pub const INVALID_SPHINX_PARAMS : &'static SphinxParams = &SphinxParams {
    protocol_name: "Invalid Sphinx!",
    beta_length: 0,
    max_beta_tail_length: 0,
    surb_log_length: 0,
    body_lengths: &[0]
};


/// A Sphinx header structured by individual components. 
///
/// Create by applying `slice_header` to `&mut [u8]` slice of the
/// correct length, like that created by `boxed_zeroed_header`.
/// We check all lengths in `slice_header` so that methods and
/// functions using `HeaderRefs` may assume all header lengths to
/// be correct.
///
/// We mostly handle `HeaderRefs` via mutable borrows so that we may
/// change referred to values without interrior mutability, but keep
/// the references tehmselves non-public to forbid changing them. 
pub struct HeaderRefs<'a> {
    /// Sphinx `'static` runtime paramaters 
    pub params: &'static SphinxParams,

    alpha: &'a mut AlphaBytes,
    gamma: &'a mut GammaBytes,
    beta:  &'a mut [u8],
    surb_log: &'a mut [u8],
    surb:  &'a mut [u8],
}

impl<'a> HeaderRefs<'a> {
    pub fn alpha(&'a self) -> &'a AlphaBytes { self.alpha }
    pub fn gamma(&'a self) -> &'a GammaBytes { self.gamma }
    pub fn beta(&'a self) -> &'a [u8] { self.beta }
    pub fn surb_log(&'a self) -> &'a [u8] { self.surb_log }
    pub fn surb(&'a self) -> &'a [u8]  { self.surb }

    pub fn alpha_mut(&'a mut self) -> &'a mut AlphaBytes { self.alpha }
    pub fn gamma_mut(&'a mut self) -> &'a mut GammaBytes { self.gamma }
    pub fn beta_mut(&'a mut self) -> &'a mut [u8] { self.beta }
    pub fn surb_log_mut(&'a mut self) -> &'a mut [u8] { self.surb_log }
    pub fn surb_mut(&'a mut self) -> &'a mut [u8]  { self.surb }

    /// Verify the poly1305 MAC `Gamma` given in a Sphinx packet by
    /// calling `SphinxHop::verify_gamma` with the provided fields.
    pub fn verify_gamma(&self, hop: SphinxHop) -> SphinxResult<()> {
        hop.verify_gamma(self.beta, self.surb, &Gamma(*self.gamma))
    }

    /// Compute gamma from Beta and the SURB.  Probably not useful.
    pub fn create_gamma(&mut self, hop: SphinxHop) {
        *self.gamma = hop.create_gamma(self.beta, self.surb).0;
    }

    pub fn prepend_to_surb_log(&mut self, prepend: &[u8]) {
        let start = prepend.len();
        let ref mut surb_log = self.surb_log;
        if surb_log.len() > start {
            for i in start .. surb_log.len() {
                surb_log[i] = surb_log[i-start];
            }
        }
        let start = ::std::cmp::min(start,surb_log.len());
        surb_log[0..start].copy_from_slice(prepend);
    }
}

// TODO: Consider using owning_refs crate to provide
// pub fn new_sliced_header(&self) -> SphinxResult<OwningHandle<Box<[u8]>,HeaderRefs>> { }
// ref.  https://kimundi.github.io/owning-ref-rs/owning_ref/struct.OwningHandle.html

/*
use std::iter::{Iterator,IntoIterator};

pub struct HeaderIter<'a> {
    offset: usize,
    header_refs: HeaderRefs<'a>,
}

impl<'a> Iterator for HeaderIter<'a> {
    type Item=u8;

    fn next(&mut self) -> Option<u8> {
        let i = self.offset;
        self.offset += 1;
        if i < ALPHA_LENGTH { return Some(self.alpha[i]) }
        i -= ALPHA_LENGTH;
        if i < GAMMA_LENGTH { return Some(self.gamma[i]) }
        i -= GAMMA_LENGTH;
        if i < self.params.beta_length { return Some(self.beta[i]) }
        i -= self.params.beta_length;
        if i < self.params.surb_log_length { return Some(self.surb_log[i]) }
        i -= self.params.surb_log_length;
        if i < self.params.surb_length { return Some(self.surb[i]) }
        i -= self.params.surb_length;
        self.offset -= 1;  None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let l = self.params.header_length();
        (l, Some(l))
    }
}

impl<'a> Iterator ExactSizeIterator for HeaderIter<'a> {
    fn len(&self) -> usize { self.params.header_length() }
    // fn is_empty(&self) -> bool { false }
}

impl<'a> IntoIterator for HeaderRefs<'a> {
    type Item=u8;
    type IntoIter = HeaderIter<'a>;
    fn into_iter(self) -> HeaderIter<'a> {
        HeaderIter { offset: 0, header_refs: self }
    }
}
*/



