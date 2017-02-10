// Copyright 2016 Jeffrey Burdges.

//! Sphinx component of Xolotl
//!
//! ...


// pub ed25519_dalek::ed25519;

pub use super::curve::{Scalar,Point}



/*

pub trait NodeInfo {
    // fn identify() -> [u8; 16];
    // fn signing_public() -> ed25519::PublicKey;
}

pub trait NodeDHInfo {
    // fn identify() -> [u8; 16];
    // fn signing_public() -> ed25519::PublicKey;
    fn dh_public(&self) -> Point;

    /// 
    fn token(&self) -> NodeToken;
}

pub struct NodeDHPublic {
    public: Point,
}

impl NodeInfo for NodePrivate {
    fn dh_public(&self) -> &Point { &self.dh_public }

    fn token(&self) -> NodeToken {
        NodeToken::generate(self.params,)
    }
}

pub struct NodePrivate {
    private: Scalar,
    token: NodeToken
}

impl NodeInfo for NodePrivate {
    fn dh_public(&self) -> Point {
    }

    fn token(&self) -> NodeToken {
    }
}

node.private
node.token

params: &'static SphinxParams, replayer: RC, 
nt: &NodeToken,  ss: &


    let alpha = alpha.decompress() ?;  // BadAlpha
    let ss: SphinxSecret = alpha.key_exchange(node.private);

    let mut hop = SphinxHop::new(params,replayer,&node.token,&ss);
    hop.verify_gamma(beta,surb,gamma) ?;  // InvalidMAC


*/


