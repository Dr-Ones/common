//! Network utilities module.
//! Provides common functionality for network nodes (drones, clients, and servers).

use serde::{Deserialize, Serialize};
use crossbeam_channel::{Receiver, Sender};
use rand::{rngs::StdRng, Rng};
use std::collections::{HashMap, HashSet};
use wg_2024::{
    controller::DroneCommand,
    network::{NodeId, SourceRoutingHeader},
    packet::{Ack, Nack, NackType, NodeType, Packet, PacketType},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerType{
    Text,
    Media,
    Communication,
    Undefined,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum SerializableMessage {
    // For all the variants, the first argument is the sender
    Default,
    ServerTypeRequest(NodeId), // argument is the message sender: the client
    ServerTypeResponse(NodeId, ServerType), // arguments are: the sender (server) and the server type
    FilesListRequest(NodeId), // argument is the message sender: the client
    FilesListResponse(NodeId, Vec<String>), // arguments are: the sender (server) and the list of files
}
impl Default for SerializableMessage {
    fn default() -> Self {
        SerializableMessage::Default
    }
}

pub enum ClientCommand {
    ServerTypeRequest(NodeId), // argument is the server we want to get the type of
    FilesListRequest(NodeId), // argument is the server we want to get the files list from
    SendPacket(Packet),
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
}

pub enum ServerCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    SetServerType(ServerType),
}

pub enum Command {
    Client(ClientCommand),
    Server(ServerCommand),
    Drone(DroneCommand),
}

/// Common network functionality shared across different node types.
/// This trait provides basic network operations that all network nodes
/// (drones, clients, and servers) need to implement.
pub trait NetworkNode {
    /// Returns the unique identifier of this network node.
    fn get_id(&self) -> NodeId;

    fn get_seen_flood_ids(&mut self) -> &mut HashSet<u64>;

    /// Returns a reference to the map of packet senders for connected nodes.
    fn get_packet_send(&mut self) -> &mut HashMap<NodeId, Sender<Packet>>;

    /// Returns a reference to the receiver channel for incoming packets.
    fn get_packet_receiver(&self) -> &Receiver<Packet>;

    /// Returns a mutable reference to the random number generator.
    fn get_random_generator(&mut self) -> &mut StdRng;

    fn handle_routed_packet(&mut self, packet: Packet);

    fn handle_command(&mut self, command: Command);

    fn handle_packet(&mut self, packet: Packet, node_type: NodeType) {
        match packet.pack_type {
            PacketType::FloodRequest(_) => self.handle_flood_request(packet, node_type),
            _ => self.handle_routed_packet(packet),
        }
    }

    /// Forwards a packet to the next hop in its routing path.
    ///
    /// # Arguments
    /// * `packet` - The packet to forward
    ///
    /// # Panics
    /// * If the next hop's sender channel is not found
    /// * If sending the packet fails
    fn forward_packet(&mut self, packet: Packet) {
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

    fn build_nack(&self, packet: Packet, nack_type: NackType) -> Packet {
        let fragment_index = match &packet.pack_type {
            PacketType::MsgFragment(fragment) => fragment.fragment_index,
            _ => 0,
        };

        let nack = Nack {
            fragment_index,
            nack_type,
        };

        let mut response = Packet {
            pack_type: PacketType::Nack(nack),
            routing_header: packet.routing_header,
            session_id: packet.session_id,
        };

        self.reverse_packet_routing_direction(&mut response);
        response
    }

    fn build_ack(&self, packet: Packet) -> Packet {
        // 1. Keep in the ack the fragment index if the packet contains a fragment
        let frag_index: u64;

        if let PacketType::MsgFragment(fragment) = &packet.pack_type {
            frag_index = fragment.fragment_index;
        } else {
            eprintln!("Error : attempt of building an ack on a non-fragment packet.");
            panic!()
        }

        // 2. Build the Aack instance of the packet to return
        let ack: Ack = Ack {
            fragment_index: frag_index,
        };

        // 3. Build the packet
        let packet_type = PacketType::Ack(ack);

        let mut packet: Packet = Packet {
            pack_type: packet_type,
            routing_header: packet.routing_header,
            session_id: packet.session_id,
        };

        // 4. Reverse the routing direction of the packet because nacks need to be sent back
        self.reverse_packet_routing_direction(&mut packet);

        // 5. Return the packet
        packet
    }

    fn handle_flood_request(&mut self, packet: Packet, node_type: NodeType) {
        // Check if the flood request should be broadcast or turned into a flood response and sent back
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone() {
            // // DEBUG
            // eprintln!(
            //     "[NODE {}] received flood request with path_trace: {:?}",
            //     self.get_id(),
            //     flood_request.path_trace
            // );
            // Take who sent this floodRequest (test and logpurposes)
            let who_sent_me_this_flood_request = flood_request.path_trace.last().unwrap().0;

            // Add self to the path trace
            flood_request.path_trace.push((self.get_id(), node_type));

            // 1. Process some tests on the drone and its neighbours to know how to handle the flood request

            // a. Check if the drone has already received the flood request
            let flood_request_is_already_received: bool = self
                .get_seen_flood_ids()
                .iter()
                .any(|id| *id == flood_request.flood_id);

            // b. Check if the drone has a neighbour, excluding the one from which it received the flood request

            // Check if the updated neighbours list is empty

            // If I have only one neighbour, I must have received this message from it and i don't have anybody else to forward it to
            let has_no_neighbour: bool = self.get_packet_send().len() == 1;

            // 2. Check if the flood request should be sent back as a flood response or broadcasted as is
            if flood_request_is_already_received || has_no_neighbour {
                // A flood response should be created and sent

                // a. Create a build response based on the build request

                let flood_response_packet =
                    self.build_flood_response(packet, flood_request.path_trace);

                // // DEBUG
                // if has_no_neighbour {
                //     eprintln!("[NODE {}] has no neighbour -> Creating flood response. flood_id: {} hops: {:?}", self.get_id(), flood_request.flood_id, flood_response_packet.routing_header.hops);
                // } else if flood_request_is_already_received {
                //     eprintln!("[NODE {}] has already received flood request -> Creating flood response. flood_id: {} hops: {:?}", self.get_id(), flood_request.flood_id, flood_response_packet.routing_header.hops);
                // }

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
            route_back.reverse(); // I don't know anything about this, but shouldn't we use reverse_packet_routing_direction?

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
    fn broadcast_packet(&mut self, packet: Packet, who_i_received_the_packet_from: NodeId) {
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

    fn reverse_packet_routing_direction(&self, packet: &mut Packet) {
        // a. create the route back using the path trace of the packet
        let mut hops_vec: Vec<NodeId> = packet.routing_header.hops.clone();

        // remove the nodes that are not supposed to receive the packet anymore (between self and the original final destination of the packet)
        hops_vec.drain(packet.routing_header.hop_index..=hops_vec.len() - 1);

        // reverse the order of the nodes to reach in comparison with the original routing header
        hops_vec.reverse();

        let route_back: SourceRoutingHeader = SourceRoutingHeader {
            hop_index: 0, // Start from the first hop
            hops: hops_vec,
        };

        // b. update the packet's routing header
        packet.routing_header = route_back;
    }

    fn add_channel(&mut self, id: NodeId, sender: Sender<Packet>) {
        let packet_send = self.get_packet_send();
        packet_send.insert(id, sender);
    }

    fn remove_channel(&mut self, id: NodeId) {
        if !self.get_packet_send().contains_key(&id) {
            log_status(
                self.get_id(),
                &format!(
                    "Error! The current node {} has no neighbour node {}.",
                    self.get_id(),
                    id
                ),
            );
            return;
        }
        self.get_packet_send().remove(&id);
    }
}

/// Helper function for consistent status logging
pub fn log_status(node_id: NodeId, message: &str) {
    println!("[NODE {}] {}", node_id, message);
}


// ------------------------------------------------------------------------------------------------------
// ----------------------------------- TESTS ------------------------------------------------------------
// ------------------------------------------------------------------------------------------------------

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

    impl NetworkNode for TestNode {
        fn get_id(&self) -> NodeId {
            self.id
        }

        fn get_seen_flood_ids(&mut self) -> &mut HashSet<u64> {
            &mut self.seen_flood_ids
        }

        fn get_packet_send(&mut self) -> &mut HashMap<NodeId, Sender<Packet>> {
            &mut self.senders
        }

        fn get_packet_receiver(&self) -> &Receiver<Packet> {
            &self.receiver
        }

        fn get_random_generator(&mut self) -> &mut StdRng {
            &mut self.rng
        }

        fn handle_routed_packet(&mut self, _packet: Packet) {
            unimplemented!()
        }

        fn handle_command(&mut self, _command: Command) {
            unimplemented!()
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
