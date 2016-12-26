// Copyright 2016 Jeffrey Burdges.

//! Xolotl DH ratchet branches
//!
//! ...

use super::MessageKey;
use ::sphinx::SphinxSecret;

use super::twig::*;

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::hash::{Hash, Hasher};

use crypto::digest::Digest;
use crypto::sha3::Sha3;


/// We keep an extra 256 bit secret symetric key associated to any
/// hash iteration ratchet, which raises our longer term security
/// to above 256 bits.  We envision only the chainning of DH ratchets 
/// to come into play against a quantum attacker, just like only the
/// root key defends an Axolotl ratchet against a quantum attacker. 
/// We claim this gives us 128 bits against a quantum attacker.
///
/// In principle, we could employ only a 128 bit extra key here, as
/// an adversary probably cannot gain enough information to deploy
/// Grover's algorithm against any given hash iteration step.
#[derive(Debug, Default, Clone, Copy)] // Hash
pub struct ExtraKey(pub [u8; 32]);

/// Use constant time equality for `ExtraKey`.  Arguably, one should not
/// provide `==` and force users to do it manually, but this seems safer.
impl PartialEq for ExtraKey {
    fn eq(&self, other: &ExtraKey) -> bool {
        ::consistenttime::ct_u8_slice_eq(&self.0, &other.0)
    }
}
impl Eq for ExtraKey { }

// impl_Display_as_hex_for_WrapperStruct!(ExtraKey);


/// Identifying name for a ratchet branch.
///
/// At 128 bits, there is a 2^64ish quantum attack to fake a ratchet
/// branch name, allowing an attacker to claim ratchet knowlege they
/// do not posses, but this should prove useless due to the MAC.
// type BranchName = [u8; 16];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BranchName(pub [u8; 16]);
// We derive all these traits partially for use in HashMap, but
// afaik no reason for PartialOrd or Ord yet, no BinaryHeap.

impl_Display_as_hex_for_WrapperStruct!(BranchName);


// In storage, branches are identified by their parent branch's name,
// called their family, along with the berry that spawned them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BranchId {
    pub family: BranchName, 
    pub berry: TwigIdx
}

impl fmt::Display for BranchId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BranchId({:x},{})", self.family, self.berry)
    }
}


/// Branchs grow from leaves and DH key exchanges.  A branch's BranchId
/// used for storage must be tracked seperately by trasaction objects.
// #[repr(packed)]
pub struct Branch {
    /// A branch stores an extra 256 bits of key material beyond
    /// the 128 bits stored in twigs to support post-quantum security.
    pub extra : ExtraKey,

    /// Index of next chainkey expected to be converted into a berry key.
    /// There can be train twigs larger than chain, but no twigs of other types.
    pub chain: TwigIdx,
}


/// A nice poem about cats for our KDFs.
const TIGER : [ &'static str; 9 ] = [
    "Little Tiger, burning bright",
    "With a subtle Blakeish light,",
    "Tell what visions have their home",
    "In those eyes of flame and chrome!",
    "Children vex thee - thoughtless, gay -",
    "Holding when thou wouldst away:",
    "What dark lore is that which thou,",
    "Spitting, mixest with thy meow?",
    "    - H.P. Lovecraft" ];

impl Branch {
    /// Family name for child branches spawned from our berries.
    pub fn child_family_name(&self) -> BranchName {
        let mut r = [0u8; 16];
        debug_assert_eq!(::std::mem::size_of_val(&r),
          ::std::mem::size_of::<BranchName>());
        let mut sha = Sha3::sha3_256();

        sha.input_str( TIGER[3] );
        sha.input(&self.extra.0);
        sha.input_str( TIGER[5] );
        sha.input(&self.extra.0);
        sha.result(&mut r);
        sha.reset();

        BranchName(r)
    }

    /// Advance a train twig
    pub fn kdf_train(&self, i: TwigIdx, ck: &TrainKey)
            -> (TrainKey, TrainKey, ChainKey, LinkKey) {
        ck.assert_twigy();

        let mut r = [0u8; 4*16];
        debug_assert_eq!(::std::mem::size_of_val(&r),
          ::std::mem::size_of::<(TrainKey, TrainKey, ChainKey, LinkKey)>());
        // let mut r: (TrainKey, TrainKey, ChainKey, LinkKey) = Default::default();
        // debug_assert_eq!(mem::size_of_val(&r), 512/8);
        let mut sha = Sha3::sha3_512();

        sha.input_str( TIGER[0] );
        sha.input(&self.extra.0);  // was self.child_family_name().0
        sha.input(&ck.0);
        sha.input_str( TIGER[1] );
        sha.input(&self.extra.0);
        sha.input_str( TIGER[2] );
        sha.input(& i.to_bytes());
        sha.input(&ck.0);
        sha.input_str( TIGER[3] );
        sha.result(&mut r);
        // sha.result( 
        //   unsafe { mem::transmute::<&mut (TrainKey, TrainKey, ChainKey, LinkKey),&mut [u8;4*16]>(&mut r) } 
        // );
        sha.reset();

        // (TrainKey(r[0..15]), TrainKey(r[16..31]),
        //  ChainKey(r[32..47]), LinkKey(r[48..63]))
        let (a,b,c,d) = array_refs![&r,16,16,16,16];
        (TrainKey::new(*a), TrainKey::new(*b), ChainKey::new(*c), LinkKey::new(*d))
        // r
    }

    /// Advance a chain twig
    pub fn kdf_chain(&self, i: TwigIdx, ck: &ChainKey)
            -> (ChainKey,LinkKey) {
        ck.assert_twigy();

        let mut r = [0u8; 2*16];
        debug_assert_eq!(::std::mem::size_of_val(&r),
          ::std::mem::size_of::<(ChainKey, LinkKey)>());
        // let mut r: (ChainKey, LinkKey) = Default::default();
        // debug_assert_eq!(mem::size_of_val(&r), 256/8);
        let mut sha = Sha3::sha3_256();

        sha.input_str( TIGER[2] );
        sha.input(&self.extra.0);  // was self.child_family_name().0
        sha.input(&ck.0);
        sha.input_str( TIGER[3] );
        sha.input(& i.to_bytes());
        sha.input(&self.extra.0);
        sha.input_str( TIGER[4] );
        sha.input(&ck.0);
        sha.input_str( TIGER[5] );
        sha.result(&mut r);
        // sha.result( 
        //   unsafe { mem::transmute::<&mut (LinkKey, ChainKey, ChainKey),&mut [u8;3*16]>(&mut r) } 
        // );
        sha.reset();

        // (ChainKey(r[0..15]), LinkKey(r[15..31]))
        let (a,b) = array_refs![&r,16,16];
        (ChainKey::new(*a), LinkKey::new(*b))
        // r
    }

    /// Sphinx berry KDF
    pub fn kdf_berry(&self, linkkey: &LinkKey, s: &SphinxSecret)
            -> (MessageKey, BerryKey) {
        linkkey.assert_twigy();

        let mut r = [0u8; 32+16];
        debug_assert_eq!(::std::mem::size_of_val(&r),
          ::std::mem::size_of::<(MessageKey, BerryKey)>());
        // let mut r: (MessageKey, BerryKey) = Default::default();
        // debug_assert_eq!(mem::size_of_val(&r), 384/8);
        let mut sha = Sha3::sha3_384();

        sha.input_str( TIGER[4] );
        sha.input(&self.extra.0);  // was self.child_family_name().0
        sha.input(&linkkey.0);
        sha.input_str( TIGER[5] );
        sha.input(&s.0);
        sha.input(&self.extra.0);
        sha.input_str( TIGER[6] );
        sha.input(&s.0);
        sha.input(&linkkey.0);
        sha.input_str( TIGER[7] );
        sha.result(&mut r);
        // sha.result(
        //   unsafe { mem::transmute::<&mut (MessageKey, BerryKey),&mut [u8;32+16]>(&mut r) } 
        // );
        sha.reset();

        // (MessageKey::new(r[0..31]), BerryKey(r[32..47]))
        let (a,b) = array_refs![&r,32,16];
        (MessageKey::new(*a), BerryKey::new(*b))
        // r
    }

    /// Produce a new branch from a berry
    pub fn kdf_branch(&self, i: TwigIdx, bk: &BerryKey)
            -> (BranchId, Branch, TrainKey) {
        bk.assert_twigy();

        let mut r = [0u8; 32+16];
        debug_assert_eq!(::std::mem::size_of_val(&r),
          ::std::mem::size_of::<(ExtraKey, TrainKey)>());
        // let mut r: (ExtraKey, TrainKey) = Default::default();
        // debug_assert_eq!(mem::size_of_val(&r), 384/8);
        let mut sha = Sha3::sha3_384();

        sha.input(&self.extra.0);
        sha.input_str( TIGER[0] );
        sha.input(&bk.0);
        sha.input_str( TIGER[2] );
        sha.input(&self.extra.0);
        sha.input_str( TIGER[4] );
        sha.input(& i.to_bytes());
        sha.input(&bk.0);
        sha.input_str( TIGER[6] );
        sha.result(&mut r);
        sha.reset();

        let (e,t) = array_refs![&r,32,16];
        ( 
            BranchId { family: self.child_family_name(), berry: i },
            Branch {
                extra: ExtraKey(*e),   // ExtraKey(r[0..31])
                chain: TRAIN_START,
            }, 
            TrainKey::new(*t)  // BranchName(r[32..47])
        )
    }

    pub fn new_kdf(seed: &[u8]) -> (BranchId, Branch, TrainKey) {
        let mut r = [0u8; 32+16+16];
        debug_assert_eq!(::std::mem::size_of_val(&r),
          ::std::mem::size_of::<(ExtraKey, BranchName, TrainKey)>());
        let mut sha = Sha3::sha3_512();

        sha.input_str( TIGER[1] );
        sha.input(seed);
        sha.input_str( TIGER[7] );
        sha.result(&mut r);
        sha.reset();

        let (e,f,t) = array_refs![&r,32,16,16];
        (
            BranchId { 
                family: BranchName(*f),  // BranchName(r[32..47]),
                berry: TwigIdx(TwigIdxT::max_value()) 
            },
            Branch {
                extra: ExtraKey(*e),  // ExtraKey(r[0..31])
                chain: TRAIN_START,
            },
            TrainKey::new(*t)  // TrainKey(r[48..63])
        )
    }
}



