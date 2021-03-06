// Copyright 2016 Jeffrey Burdges.

//! Sphinx header asymmetric cryptographic routines
//!
//! ...

use std::fmt;

use rand::{Rng, Rand};

use curve25519_dalek::scalar;
use curve25519_dalek::edwards;
use curve25519_dalek::constants;
// ::{CompressedEdwardsY,ExtendedPoint,ScalarMult,BasepointMult};

use sphinx::SphinxSecret;
use sphinx::error::*;


/// A curve25519 scalar chosen uniformly from ℤ/lℤ where
/// 
/// l = 2^252 + 27742317777372353535851937790883648493
///
/// Sphinx uses `Scalar` for node prigate keys, packet private keys,
/// and blinding factors.  
/// 
/// Warning: We employ a reduction mod l when blinding packet private keys,
/// so packet private key cannot be represented as an element of ℤ/8lℤ
/// with low bits zero like normal curve25519 private keys.  Instead, we
/// must judiciously multiply by the cofactor when doing any key exchage.
#[derive(Copy,Clone)]
pub struct Scalar(scalar::Scalar);

pub type ScalarBytes = [u8; 32];

impl Scalar {
    /// Create a curve25519 scalar for Sphinx by reducing a specified
    /// 512 bit seed mod l.  The seed should be chose reasonably uniformly
    /// from `[u8; 64]` so that the result is nearly uniform in ℤ/lℤ.
    pub fn make(seed: &[u8; 64]) -> Scalar {
        // We're being excessive by using a reduction mod l from a huge
        // number here, but doing so instead of zeroing high bits might
        // discurage future developers from "claimping" like for public
        // keys.  Zeroing the low bits or setting the high bit create a
        // vulnerability against a quantum attacker.
        Scalar(scalar::Scalar::reduce(seed)) 
    }

    /// Return the scalar's standard byte representation for saving to disk.
    pub fn to_bytes(&self) -> ScalarBytes {
        let scalar::Scalar(s) = self.0;
        s
    }

    /// Construct a scalar loaded from its byte representation loaded from
    /// disk.  Do not create a blinding scalar with this functon.
    pub fn from_bytes(s: &ScalarBytes) -> Scalar {
        Scalar(scalar::Scalar(*s))
    }

    pub fn blind(&mut self, other: &Scalar) {
        let s = scalar::Scalar::multiply_add(& self.0, &other.0, & scalar::Scalar::zero());
        *self = Scalar(s);
    }
}

impl Rand for Scalar {
    fn rand<R: Rng>(rng: &mut R) -> Scalar {
        let mut s = [0u8; 64];
        rng.fill_bytes(&mut s);
        Scalar::make(&s)
    }
}


impl fmt::Debug for Scalar {
    #[cfg(not(test))]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Scalar(scrubbed)")
    }

    #[cfg(test)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use hex::ToHex;
        let scalar::Scalar(s) = self.0;
        write!(f, "Scalar({:})", s.to_hex())
    }
}



pub const ALPHA_LENGTH : usize = 32;

/// Sphinx packet or node public key consisting of a curve25519 point
/// represented in the compressed Edwards Y cordinate form given by
/// `edwards::CompressedEdwardsY`, as opposed to the compressed Montgomery U
/// form for the `curve25519()` Diffie-Hellman key exchange function
/// Sphinx usually uses.
pub type AlphaBytes = [u8; ALPHA_LENGTH];


/// Sphinx packet or node public key consisting of a curve25519 point
/// represented as an `edwards::ExtendedPoint` in 𝗣³(𝔽ₚ) for efficent computations.
///
/// Warning: A `Point` almost always represnets public key material supplied
/// by another parts, so one must multiply by the cofactor 8 when combining
/// with a secret key scalar.  A normal curve25519 implementation used scalars
/// in ℤ/8lℤ with their low three bits zeroed to achieve this, but we require
/// scalars in ℤ/lℤ where l = 2^252 + 27742317777372353535851937790883648493.
#[derive(Clone)] // Copy, PartialEq, Eq, Hash
pub struct Point(edwards::ExtendedPoint);

impl Point {
    /// Create a packet or node public key from a secret key scalar.
    pub fn from_secret(s: &Scalar) -> Point {
        Point( &constants::ED25519_BASEPOINT_TABLE * &s.0 )
    }

    /// Compress a point into bytes for either transmission as a
    /// public key or local storage.
    ///
    /// Do not use this for key exchange.
    pub fn compress(&self) -> AlphaBytes {
        self.0.compress_edwards().to_bytes()
    }

    /// Decompress a point supplied in compressed Edwards Y cordinate
    /// form from a Sphinx packet's Alpha or a routing public key. 
    /// As the point is untrusted, we must multiply by the cofactor to
    /// protect against small subgroup attacks.  We never expect to
    /// decompress our own key in deployment, but tests might fail if
    /// one does so in testing.
    ///
    /// TODO: Add validation 
    /// See https://research.kudelskisecurity.com/2017/04/25/should-ecdh-keys-be-validated/
    pub fn decompress(alpha_bytes: &AlphaBytes) -> SphinxResult<Point> {
        // let f = if trusted { |p| Point(p) } else { |p| Point(p.mult_by_cofactor()) };
        edwards::CompressedEdwardsY(*alpha_bytes).decompress()
            .map(|p| Point(p.mult_by_cofactor()))
            .ok_or( SphinxError::BadAlpha(*alpha_bytes) )
    }

    /// Sphinx protocol multiplication of a packet public key `Point` by
    /// a blinding factor `Scalar`.
    ///
    /// Warning: We do not validate the packet public key by multipling
    /// it by cofactor 8 here, so this should only be used when we know
    /// both parties know the blinding factor.
    pub fn blind(&self, blinding: &Scalar) -> Point {
        Point( &self.0 * &blinding.0 )
    }

    /// Multiply a curve25519 public key `Point` by a private key `Scalar`
    /// to preform a key exchange. 
    ///
    /// We took scalars to be elements of ℤ/lℤ rather than ℤ/8lℤ, so that
    /// we could reduce private scalars mod l freely.  As a result, we must
    /// multiply by the cofactor to prevent small subgroup attacks.  Also,
    /// we neglected to set the high bit of our scalars, as a measure
    /// against quantum adversaries who can observe blinding factors, so
    /// require that the scalar multiplicaiton operation has constant time.
    pub fn key_exchange(&self, private_key: &Scalar) -> SphinxSecret {
        SphinxSecret(
            ( &self.0 * &private_key.0 )
                .compress_edwards().to_bytes()
        )
    }
}

impl fmt::Debug for Point {
    #[cfg(not(test))]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Point(scrubbed)")
    }

    #[cfg(test)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use hex::ToHex;
        write!(f, "Point({:})", self.compress().to_hex())
    }
}


#[cfg(test)]
mod tests {
    use crypto::curve25519 as rc_curve25519;

    use rand::{OsRng, Rng, Rand};
    use super::*;
    // use hex::ToHex;

    fn os_rng() -> OsRng {
        OsRng::new().expect("failed to create an OS RNG")
    }

    #[test]
    fn test_scalar_reduce() {
        let mut r = os_rng();
        for _ in 0..10 {
            let mut a = [0u8; 64];   r.fill_bytes(&mut a);
            let Scalar(scalar::Scalar(c)) = Scalar::make(&a);
            let scalar::Scalar(b) = scalar::Scalar::reduce(&a);
            rc_curve25519::sc_reduce(&mut a);
            assert_eq!(b,a[0..32]);
            assert_eq!(b,c);
        }
    }

    /*
    fn rand_scalar<R: Rng>(rng: &mut R) -> Scalar {
        let mut s = [0u8; 64];
        rng.fill_bytes(&mut s);
        Scalar::make
        scalar::Scalar(s)
    }
    */

    #[test]
    fn test_scalar_multiply_add() {
        let mut r = os_rng();
        for _ in 0..10 {
            let a = scalar::Scalar(Rand::rand(&mut r));
            let b = scalar::Scalar(Rand::rand(&mut r));
            let c = scalar::Scalar(Rand::rand(&mut r));
            let mut x = [0u8; 32];
            rc_curve25519::sc_muladd(&mut x,&a.0,&b.0,&c.0);
            let scalar::Scalar(y) = scalar::Scalar::multiply_add(&a,&b,&c);
            assert_eq!(x,y);
        }
    }

    fn scalar_mul_by_cofactor(b: &mut [u8; 32]) {
        let mut r = 0;
        for i in 0..31 {
            let mut t = b[i] as u16;
            t <<= 3;
            t |= r;
            b[i] = (t & 0xFF) as u8;
            r = t >> 8;
        }
    }

    #[test]
    fn test_curve_scalarmult() {
        let mut r = os_rng();
        for i in 0..5 {
            // Psedu-reduce the scalar by clearing the high bit or actually reducing it.
            let a = if i % 2 == 1 {
                let mut s = scalar::Scalar(Rand::rand(&mut r));
                // s[0]  &= 248;  // Avoids small subgroup attack by multiplying by cofactor
                s[31] &= 127;  // Zeros high bits
                // s[31] |=  64;  // Sets high bit for constant time, probably unnecessary.
                s
            } else {
                Scalar::rand(&mut r).0
            };
            let x = rc_curve25519::ge_scalarmult_base(&a.0);
            let y = (&constants::ED25519_BASEPOINT_TABLE * &a).compress_edwards().to_bytes();
            assert_eq!(x.to_bytes(),y);
            let z = Point::from_secret(&Scalar(a));
            assert_eq!(y,z.compress());

            let mut b = Scalar::rand(&mut r).0;
            // let z = edwards::ExtendedPoint::scalar_mult(??,&s).compress_edwards().to_bytes();
            // assert_eq!(z,y);

            let zero = scalar::Scalar::zero();
            let u = rc_curve25519::GeP2::double_scalarmult_vartime(&b.0, x, &zero.0);
            let v = z.blind(&Scalar(b));
            assert_eq!(u.to_bytes(), v.compress());

            let w = v.0.compress_edwards().to_bytes();
            assert_eq!(w, z.key_exchange(&Scalar(b)).0);

            scalar_mul_by_cofactor(&mut b.0);
            // assert_eq!(w, z.blind(&Scalar(b)).compress());
        }
    }

/*

decompress()
.from_bytes_negate_vartime()

    #[test]
    fn test_curve_scalar_mult() {
        let mut r = os_rng();
        for _ in 0..10 {
curve::Curve::scalar_mult
rc_curve25519::double_scalarmult_vartime
            assert_eq!(x,y);
        }
    }


    #[test]
    fn test_curve_scalar_mult_double() {
        let mut r = os_rng();
        for _ in 0..10 {
curve::double_scalar_mult_vartime
rc_curve25519::double_scalarmult_vartime
            assert_eq!(x,y);
        }
    }
*/


/*
    #[test]
    fn need_test_() {
    }
*/
}




