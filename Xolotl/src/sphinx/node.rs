// Copyright 2016 Jeffrey Burdges.

//! Sphinx component of Xolotl
//!
//! ...


// pub ed25519_dalek::ed25519;


use super::curve::{AlphaBytes,Scalar,Point};
use super::stream::{Gamma,SphinxKey,SphinxHop};
use super::header::{SphinxParams,HeaderRefs,Command};
use super::keys::RoutingName;
use super::mailbox::MailboxName;
use super::utils::*;
use super::error::*;
use super::*;


/// Action the node should take with a given packet.
/// We pair PacketName
enum Action {
    /// Deliver message to a local mailbox
    Deliver {
        /// Mailbox name
        mailbox: MailboxName,
        ///
        surb_log: Box<[u8]>,
    },

    /// Forward this message to another hop.
    Transmit {
        /// Next hop
        route: RoutingName,
    },

    /// Arrival of a message for some local application.
    ///
    /// There are situations where we could know the sender because
    /// either we could know who we gave every SURB to, or else by
    /// trusting them to identify themselves as a hint and doing the
    /// authentication later.  Yet, these cases seem tricky to exploit.
    Arrival {
        surbs: Vec<PacketName>,
    },
}


struct SphinxRouter {
    params: &'static SphinxParams,

    // routing_public: RoutingPublic,
    routing_secret: RoutingSecret,

    replayer: RwLock<Filter<Key=ReplayCode>>,

    outgoing: OutgoingStore,
    mailboxes: MailboxStore,
    arrivals: ArrivingStore,
}


impl SphinxRouter {
    /// Process an incoming Sphinx packet cut into references.
    /// Invokes ratchet and cross over functionality itself, but
    /// must return an `Action` for functionality that requires
    /// ownership of the header and/or body.
    fn do_crypto(&self, refs: HeaderRefs, body: &mut [u8]) -> SphinxResult<()> {
        // Compute shared secret from the Diffie-Helman key exchange.
        let alpha = refs.alpha.decompress() ?;  // BadAlpha
        let ss = alpha.key_exchange(self.private);

        // Initalize the stream cipher
        let mut key = self.params.sphinx_key(&ss, self.routing_secret.name);
        let mut hop = key.hop();

        // Abort if our MAC gamma fails to verify
        refs.verify_gamma(&hop) ?;  // InvalidMac

        // Abort if the packet is a reply
        hop.replay_check(&self.replayer) ?; // Replay

        // Onion decrypt beta to extract first command.
        let mut command = refs.peal_beta(&mut hop);  

        // Process `Command::Ratchet` before decrypting the surb log or body.
        if Command::Ratchet { twig, gamma } = command {
            let TwigId(branch_id, twig_idx) = *twig;
            let advance = AdvanceNode::(state, &twig, &branch_id) ?;  // RatchetError ??
            key.chacha_key = advance.clicks(&ss, twig_idx) ?;  // RatchetError ??
            // Reinitalize 
            hop = key.hop();
            *refs.gamma = gamma;
            if ! refs.verify_gamma(&hop) {
                advance.abandon() ?;  // RatchetError ??
                return hop.invalid_mac_error();  // InvalidMac
            }
            advance.confirm() ?;  // RatchetError ??
            command = refs.peal_beta(&mut hop);  // InternalError
            if Ratchet { .. } = command {
                return Err( SphinxError::BadPacket("Tried two ratchet subhops.") );
            }
        }

        // No need to constant time here.  Should just pass the bool really.
        let already_crossed_over = ::consistenttime::ct_u8_eq( 0u8, 
            ref.surb_log.iter().fold(0u8, |x,y| { x |= *y }) 
        );

        // Short circut decrypting the body, SURB and SURB log if
        // we're unwinding an arriving SURB anyways.
        // TODO: Should we better authenticate that SURB were created by us?
        if Command::ArrivalSURB { } = command {
            // hop.xor_surb(refs.surb);
            // hop.xor_surb_log(refs.surb_log);
            // hop.lionness_cipher().decrypt(body) ?;  // InternalError 
            return self.unwind_surbs(hop.packet_name(), refs.surb_log, body, true);
        }

        // Decrypt body
        hop.lionness_cipher().decrypt(body) ?;  // InternalError 

        Ok(( hop.packet_name(), match command {
            Command::ArrivalSURB { } => unreachable!(),
            Command::Ratchet {..} => unreachable!(),

            // We cross over to running the SURB by moving the SURB
            // into postion and recursing. 
            Command::CrossOver { alpha, gamma } => {
                if already_crossed_over {
                    return Err( SphinxError::BadPacket("Tried two crossover subhops.") );
                }
                // Put SURB in control of packet.
                *refs.alpha = alpha;
                *refs.gamma = gamma;
                *refs.beta.copy_from_slice(refs.surb);
                // We must zero the SURB feld so that our SURB's gammas
                // cover values known by its creator.  We might improve
                // SURB unwinding by zeroing the SURB log field too. 
                // These two fields are safe to zero now because they 
                // will immediately be encrypted.
                for i in refs.surb.iter_mut() { *i = 0; }
                for i in refs.surb_log.iter_mut() { *i = 0; }
                // Process the local SURB hop.
                self.do_crypto(refs,body)
            },

            // We mutate all `refs.*` in place, along with body, so
            // `Transmit` merely drops this mutable borrow of the 
            // header and queues the now mutated header and body.
            // As a result, we require that both the original header 
            // and SURB use the same protocol version as specified 
            // by `self.params`.
            //
            // Also, we must never change the referant of any of our 
            // references in `refs`, even though `refs` must itself 
            // be mutable.  We may fix this with either simple guards
            // types or smaybe interior mutability depneding upon how
            // this code evolves. 
            Command::Transmit { route, gamma } => {
                // Only transmit need to mask the SURB.
                hop.xor_surb(refs.surb);
                hop.xor_surb_log(refs.surb_log);
                // Prepare packet for next hop as usual in Sphinx.
                *refs.gamma = gamma;
                *refs.alpha = alpha.blind(& (hop.blinding() ?)).compress();
                Action::Transmit { route }
            },

            // We box the SURB log because we must store it for pickup
            // via SURB.  At that time, we embed the packet name with
            // roughly `refs.prepend_to_surb_log(& hop.packet_name());`
            Command::Deliver { mailbox } =>
                Action::Delivery { mailbox, surb_log: refs.surb_log.to_vec().into_boxed_slice() }

            Command::ArrivalDirect { } =>
                Action::Arrival { surbs: vec![] },
        } ))
    }

    /// Process an incoming Sphinx packet.
    pub fn process(&self, header: Box<[u8]>, body: Box<[u8]>)
      -> SphinxResult<()>
    {
        assert!(self.params.max_beta_tail_length < self.params.beta_length);
        // assert lengths ...

        self.params.check_body_length(body) ?; // BadLength
        let (packet, action) = {
            let refs = self.params.slice_header(header.borrow_mut()) ?;  // BadLength
            self.do_crypto(kdf,refs,body.borrow_mut()) ? 
        };
        match action {
            Action::Transmit { route } =>
                self.outgoing.enqueue(&route, packet, OutgoingPacket { route, header, body } ),
            Action::Deliver { mailbox, surb_log } =>
                self.mailboxes.enqueue(&mailbox, packet, MailboxPacket { surb_log, body } ),
            Action::Arrival { surbs } =>
                self.arrivals.push( ArivingPacket { surbs, body } ),
        }
    }

    /// Unwind a chain of SURBs
    fn unwind_surbs(&self, mut packet_name: PacketName, mut surb_log: &mut [u8], body: &mut [u8]) -> SphinxResult<()> 
    {
        let mut starting = true;
        let cap = surb_log.len() / PACKET_NAME_LENGTH + 1;
        let mut purposes = Vec::<PacketName>::with_capacity(cap);
        // TODO: Should we authentcate SURBs better?
/*
        loop {
            let surb_hop = { 
                let surb_archive = self.surb_archive.write() ?;  // PoisonError
                if let Some(sh) = surb_archive.remove(packet_name) { *sh } else {
                    // If any SURBs existed 
                    if ! starting { break; }
                    return SphinxError::BadSURBPacketName;
                }
            };
            if ! starting {
                let key = self.params.sphinx_key(&ss, self.routing_secret.name);
                let mut hop = key.hop();
                hop.xor_surb_log(surb_log) ?;  // InternalError
                hop.lionness_cipher().encrypt(body) ?;  // InternalError
            } else {
                purposes.push(packet_name);
                starting = false; 
            }

            packet_name = if surb_hop.preceeding != PacketName::default() {
                surb_hop.preceeding
            } else if surb_log.len() >= PACKET_NAME_LENGTH {
                PacketName(*reserve_fixed!(surb_log, PACKET_NAME_LENGTH))
            } else { break; };
            if packet_name == PacketName::default() { break; }
        }

        let surb_archive = self.surb_archive.write() ?;  // PoisonError
        ;
*/

        return Action::Arrival { surbs: purposes } 
    }

}


