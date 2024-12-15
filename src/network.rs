//! Network utilities module.
//! Provides common functionality for network nodes (drones, clients, and servers).

use crossbeam_channel::Sender;
use rand::rngs::StdRng;
use std::collections::HashMap;
use wg_2024::{network::NodeId, packet::Packet};

/// Common network functionality shared across different node types.
/// This trait provides basic network operations that all network nodes
/// (drones, clients, and servers) need to implement.
pub trait NetworkUtils {
    /// Returns the unique identifier of this network node.
    fn get_id(&self) -> NodeId;

    /// Returns a reference to the map of packet senders for connected nodes.
    fn get_packet_senders(&self) -> &HashMap<NodeId, Sender<Packet>>;

    /// Returns a mutable reference to the random number generator.
    fn get_random_generator(&mut self) -> &mut StdRng;

    /// Forwards a packet to the next hop in its routing path.
    ///
    /// # Arguments
    /// * `packet` - The packet to forward
    ///
    /// # Panics
    /// * If the next hop's sender channel is not found
    /// * If sending the packet fails
    fn forward_packet(&self, packet: Packet) {
        let next_hop_id = packet.routing_header.hops[packet.routing_header.hop_index];

        if let Some(sender) = self.get_packet_senders().get(&next_hop_id) {
            sender.send(packet).expect("Failed to forward the packet");
        } else {
            log_status(
                self.get_id(),
                &format!("No channel found for next hop: {:?}", next_hop_id),
            );
        }
    }
}

/// Helper function for consistent status logging
pub fn log_status(node_id: NodeId, message: &str) {
    println!("[NODE {}] {}", node_id, message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use rand::SeedableRng;

    struct TestNode {
        id: NodeId,
        senders: HashMap<NodeId, Sender<Packet>>,
        rng: StdRng,
    }

    impl NetworkUtils for TestNode {
        fn get_id(&self) -> NodeId {
            self.id
        }

        fn get_packet_senders(&self) -> &HashMap<NodeId, Sender<Packet>> {
            &self.senders
        }

        fn get_random_generator(&mut self) -> &mut StdRng {
            &mut self.rng
        }
    }

    impl TestNode {
        fn new(id: NodeId) -> Self {
            Self {
                id,
                senders: HashMap::new(),
                rng: StdRng::from_entropy(),
            }
        }
    }

    #[test]
    fn test_forward_packet() {
        // Create a test node with ID 1
        let mut node = TestNode::new(1);

        // Set up communication channel to node 2
        let (sender, receiver) = unbounded();
        node.senders.insert(2, sender);

        // Create a test packet
        // The routing path is [1, 2] and we're at node 1 (index 0)
        // trying to forward to node 2
        let packet = Packet {
            pack_type: wg_2024::packet::PacketType::Ack(wg_2024::packet::Ack { fragment_index: 0 }),
            routing_header: wg_2024::network::SourceRoutingHeader {
                hop_index: 1,
                hops: vec![1, 2],
            },
            session_id: 42,
        };

        // Test forwarding the packet
        // Current node is 1 (hops[0]), should forward to 2 (hops[1])
        node.forward_packet(packet.clone());

        // Verify the packet was received
        let received = receiver.try_recv().unwrap();
        assert_eq!(received.session_id, 42);
    }
}
