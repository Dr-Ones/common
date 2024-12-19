//! Network utilities module.
//! Provides common functionality for network nodes (drones, clients, and servers).

use rand::Rng;
use crossbeam_channel::{Receiver, Sender};
use rand::rngs::StdRng;
use std::collections::{HashMap, HashSet};
use wg_2024::{network::{NodeId, SourceRoutingHeader}, packet::{Packet, NodeType, FloodResponse, FloodRequest}};
use wg_2024::packet::PacketType;

/// Common network functionality shared across different node types.
/// This trait provides basic network operations that all network nodes
/// (drones, clients, and servers) need to implement.
pub trait NetworkUtils {
    /// Returns the unique identifier of this network node.
    fn get_id(&self) -> NodeId;

    fn get_seen_flood_ids(&mut self) -> &mut HashSet<u64>;

    /// Returns a reference to the map of packet senders for connected nodes.
    fn get_packet_send(&self) -> &HashMap<NodeId, Sender<Packet>>;

    /// Returns a reference to the receiver channel for incoming packets.
    fn get_packet_receiver(&self) -> &Receiver<Packet>;

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

        if let Some(sender) = self.get_packet_send().get(&next_hop_id) {
            sender.send(packet).expect("Failed to forward the packet");
        } else {
            log_status(
                self.get_id(),
                &format!("No channel found for next hop: {:?}", next_hop_id),
            );
        }
    }

    fn handle_flood_request(&mut self, packet: Packet, nodeType:NodeType) {
        // Check if the flood request should be broadcast or turned into a flood response and sent back
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone() {
            // Take who sent this floodRequest (test and logpurposes)
            let who_sent_me_this_flood_request = flood_request.path_trace.last().unwrap().0;

            // Add self to the path trace
            flood_request.path_trace.push((self.get_id(), nodeType));

            // 1. Process some tests on the drone and its neighbours to know how to handle the flood request

            // a. Check if the drone has already received the flood request
            let flood_request_is_already_received: bool = self.get_seen_flood_ids()
                .iter()
                .any(|id| *id == flood_request.flood_id);

            // b. Check if the drone has a neighbour, excluding the one from which it received the flood request

            // Check if the updated neighbours list is empty

            // If I have only one neighbour, I must have received this message from it and i don't have anybody else to forward it to
            let has_no_neighbour: bool = self.get_packet_send().len() == 1;

            // 2. Check if the flood request should be sent back as a flood response or broadcast as is
            if flood_request_is_already_received || has_no_neighbour {
                // A flood response should be created and sent

                // a. Create a build response based on the build request

                let flood_response_packet =
                    self.build_flood_response(packet, flood_request.path_trace);

                // b. forward the flood response back
                // eprintln!(
                //     "[CLIENT {}] Sending FloodResponse sess_id:{} whose path is: {:?}",
                //     self.id,
                //     flood_response_packet.session_id,
                //     flood_response_packet.routing_header.hops
                // );
                self.forward_packet(flood_response_packet);
            } else {
                // The packet should be broadcast
                // eprintln!("Drone id: {} -> flood_request with path_trace: {:?} broadcasted to peers: {:?}", self.id, flood_request.path_trace, self.packet_send.keys());
                self.get_seen_flood_ids().insert(flood_request.flood_id);

                // Create the new packet with the updated flood_request
                let updated_packet = Packet {
                    pack_type: PacketType::FloodRequest(flood_request),
                    routing_header: packet.routing_header,
                    session_id: packet.session_id,
                };

                // Broadcast the updated packet
                self.broadcast_packet(updated_packet, who_sent_me_this_flood_request);
            }
        } else {
            eprintln!("Error: the packet to be broadcast is not a flood request.");
        }
    }

    fn build_flood_response(
        &mut self,
        packet: Packet,
        path_trace: Vec<(NodeId, NodeType)>,
    ) -> Packet {
        if let PacketType::FloodRequest(flood_request) = packet.pack_type {
            let mut route_back: Vec<NodeId> = path_trace.iter().map(|tuple| tuple.0).collect();
            route_back.reverse();

            let new_routing_header = SourceRoutingHeader {
                hop_index: 1,
                hops: route_back,
            };

            Packet {
                pack_type: PacketType::FloodResponse(wg_2024::packet::FloodResponse {
                    flood_id: flood_request.flood_id,
                    path_trace,
                }),
                routing_header: new_routing_header,
                session_id: self.get_random_generator().gen(),
            }
        } else {
            panic!("Error! Attempt to build flood response from non-flood request packet");
        }
    }


    // forward packet to a selected group of nodes in a flooding context
    fn broadcast_packet(&self, packet: Packet, who_i_received_the_packet_from: NodeId) {
        // Copy the list of the neighbours and remove the neighbour drone that sent the flood request
        let neighbours: HashMap<NodeId, Sender<Packet>> = self
            .get_packet_send()
            .iter()
            .filter(|(&key, _)| key != who_i_received_the_packet_from)
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        // iterate on the neighbours list
        for (&node_id, sender) in neighbours.iter() {
            // Send a clone packet
            if let Err(e) = sender.send(packet.clone()) {
                println!("Failed to send packet to NodeId {:?}: {:?}", node_id, e);
            }
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
        seen_flood_ids: HashSet<u64>,
        senders: HashMap<NodeId, Sender<Packet>>,
        receiver: Receiver<Packet>,
        rng: StdRng,
    }

    impl NetworkUtils for TestNode {
        fn get_id(&self) -> NodeId {
            self.id
        }

        fn get_seen_flood_ids(&mut self) -> &mut HashSet<u64> {
            &mut self.seen_flood_ids
        }

        fn get_packet_send(&self) -> &HashMap<NodeId, Sender<Packet>> {
            &self.senders
        }

        fn get_packet_receiver(&self) -> &Receiver<Packet> {
            &self.receiver
        }

        fn get_random_generator(&mut self) -> &mut StdRng {
            &mut self.rng
        }
    }

    impl TestNode {
        fn new(id: NodeId) -> Self {
            Self {
                id,
                seen_flood_ids: HashSet::new(),
                senders: HashMap::new(),
                receiver: unbounded().1,
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
