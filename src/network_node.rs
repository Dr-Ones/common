//! Network utilities module.
//! Provides common functionality for network nodes (drones, clients, and servers).

use crossbeam_channel::{Receiver, Sender};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use wg_2024::{
    controller::{DroneCommand, DroneEvent},
    network::{NodeId, SourceRoutingHeader},
    packet::{Ack, Nack, NackType, NodeType, Packet, PacketType},
};

use crate::{log_error, log_status};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerType {
    Content,
    Communication,
    Undefined,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SerializableMessage {
    // For all the variants, the first argument is the sender
    Default,
    ServerTypeRequest(NodeId),              // argument is the sender id (client)
    ServerTypeResponse(NodeId, ServerType), // arguments are: the sender id (server) and the server type
    FileListRequest(NodeId),                // argument is the sender id (client)
    FileListResponse(NodeId, Vec<String>),  // arguments are: the sender id (server) and the list of files
    FileRequest(NodeId, String),            // argument are: the sender id (client) and the name of the requested file
    FileFound(NodeId, String, String),      // argument are: the sender id (server), the filename and the file
    RegisterToCommunicationServer(NodeId),  // argument is the sender id (client)
    RegisterSuccess(NodeId),                // argument is the sender id (server)
    ClientListRequest(NodeId),              // argument is the sender id (client)
    ClientListResponse(NodeId, Vec<NodeId>),// arguments are: the sender id (server) and the list of clients
    Chat(NodeId, NodeId, NodeId, String),   // arguments are: the sender id (client), the id of server the chat is sent on, the recipient client id and the chat text message
    ErrorMessage(NodeId, String),           // argument are: the sender id (server) and the error message
}

impl Default for SerializableMessage {
    /// Returns the default variant of `SerializableMessage`, which is `Default`.
    fn default() -> Self {
        SerializableMessage::Default
    }
}

pub enum ClientCommand {
    ServerTypeRequest(NodeId),             // argument is the id of the server we want to get the type of
    AllServerTypesRequest(),
    FileListRequest(NodeId),               // argument is the id of the server we want to get the file list from
    FileRequest(NodeId, String),           // arguments are the id of the server we want to get the file from and the name of the requested file
    RegisterToCommunicationServer(NodeId), // argument is the id of the communication server we want to register to
    Chat(NodeId, NodeId, String),          // argument are the id of the communication server we want to chat on, the id of the recipient client and the message to send
    ClientListRequest(NodeId),             // argument is the id of the server we want to get the client list from
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
    /// Retrieves the unique identifier of this network node.
    fn get_id(&self) -> NodeId;
    
    /// Indicates whether the node is set to exhibit crashing behavior.
    /// The default implementation returns `false`.
    fn get_crashing_behavior(&self) -> bool {
        return false;
    }
    
    /// Provides a mutable reference to the set of flood request IDs that have already been seen.
    /// This helps to avoid reprocessing duplicate flood requests.
    fn get_seen_flood_ids(&mut self) -> &mut HashSet<String>;
    
    /// Returns a mutable reference to the mapping of node IDs to their sender channels.
    /// This map represents the outgoing communication channels for this node.
    fn get_packet_send(&mut self) -> &mut HashMap<NodeId, Sender<Packet>>;
    
    /// Returns a reference to the channel used for receiving incoming packets.
    fn get_packet_receiver(&self) -> &Receiver<Packet>;
    
    /// Returns a mutable reference to the node's random number generator.
    fn get_random_generator(&mut self) -> &mut StdRng;
    
    /// Returns a reference to the simulation controller's sender channel for dispatching events.
    fn get_sim_contr_send(&self) -> &Sender<DroneEvent>;
    
    /// Processes a routed packet arriving at this node.
    ///
    /// # Arguments
    ///
    /// * `packet` - The packet to be processed.
    ///
    /// # Returns
    ///
    /// A boolean indicating whether the packet was successfully handled.
    fn handle_routed_packet(&mut self, packet: Packet) -> bool;
    
    /// Handles an incoming command directed to this node.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to be executed.
    fn handle_command(&mut self, command: Command);
    
    /// Determines how to process an incoming packet based on its type and the node type.
    ///
    /// For flood requests, it may trigger a flood response or broadcast the request further.
    /// For all other packets, it delegates processing to `handle_routed_packet`.
    ///
    /// # Arguments
    ///
    /// * `packet` - The packet to be handled.
    /// * `node_type` - The type of the node handling the packet.
    ///
    /// # Returns
    ///
    /// A boolean status resulting from the packet handling.
    fn handle_packet(&mut self, packet: Packet, node_type: NodeType) -> bool {
        match packet.pack_type {
            PacketType::FloodRequest(_) => {
                if self.get_crashing_behavior() {
                    true;
                }
                self.handle_flood_request(packet, node_type);
                false
            }
            _ => self.handle_routed_packet(packet),
        }
    }
    
    /// Forwards a packet to the next hop specified in the routing header.
    ///
    /// Before forwarding, a simulation event is sent. If the sender channel for the next hop
    /// is not found, the event is logged.
    ///
    /// # Arguments
    ///
    /// * `packet` - The packet to be forwarded.
    ///
    /// # Panics
    ///
    /// Panics if sending the packet fails.
    fn forward_packet(&mut self, packet: Packet) {
        let next_hop_id = packet.routing_header.hops[packet.routing_header.hop_index];
        
        if let Some(sender) = self.get_packet_send().clone().get(&next_hop_id) {
            // Send PacketSent event before forwarding
            if let Err(e) = self
                .get_sim_contr_send()
                .send(DroneEvent::PacketSent(packet.clone()))
            {
                log_error!(self.get_id(), "Failed to send PacketSent event: {:?}", e);
            }
            sender.send(packet).expect("Failed to forward the packet");
        } else {
            log_status!(
                self.get_id(),
                "No channel found for next hop: {:?}",
                next_hop_id
            );
        }
    }
    
    /// Constructs a negative acknowledgement (Nack) packet in response to a given packet.
    ///
    /// The Nack includes the fragment index from the original packet (if applicable) and
    /// reverses the packet's routing direction to send the Nack back.
    ///
    /// # Arguments
    ///
    /// * `packet` - The original packet prompting the Nack.
    /// * `nack_type` - The type of Nack to be generated.
    ///
    /// # Returns
    ///
    /// A new packet representing the Nack.
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
    
    /// Constructs an acknowledgement (Ack) packet corresponding to a message fragment packet.
    ///
    /// The function extracts the fragment index from the original packet, builds an Ack,
    /// reverses the routing direction, and returns the new packet.
    ///
    /// # Arguments
    ///
    /// * `packet` - The original fragment packet to acknowledge.
    ///
    /// # Panics
    ///
    /// Panics if the provided packet is not a fragment packet.
    fn build_ack(&self, packet: Packet) -> Packet {
        // 1. Keep in the ack the fragment index if the packet contains a fragment
        let frag_index: u64;
        
        if let PacketType::MsgFragment(fragment) = &packet.pack_type {
            frag_index = fragment.fragment_index;
        } else {
            eprintln!("Error : attempt of building an ack on a non-fragment packet.");
            panic!()
        }
        
        // 2. Build the Ack instance of the packet to return
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
        
        // 4. Reverse the routing direction of the packet because acks need to be sent back
        self.reverse_packet_routing_direction(&mut packet);
        
        // 5. Return the packet
        packet
    }
    
    /// Processes a flood request packet.
    ///
    /// Depending on whether the flood request has been seen before or if there are no other neighbours,
    /// the function either builds a flood response or broadcasts the flood request to eligible neighbours.
    ///
    /// # Arguments
    ///
    /// * `packet` - The flood request packet to handle.
    /// * `node_type` - The type of the node processing the request.
    fn handle_flood_request(&mut self, packet: Packet, node_type: NodeType) {
        // Check if the flood request should be broadcast or turned into a flood response and sent back
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone() {
            let who_sent_me_this_flood_request = flood_request.path_trace.last().expect("Failed to get the last node of the path").0;
            
            // Add self to the path trace
            flood_request.path_trace.push((self.get_id(), node_type));
            
            // 1. Process some tests on the node and its neighbours to know how to handle the flood request
            
            // a. Check if the node has already received the flood request
            let flood_request_is_already_received: bool = self
                .get_seen_flood_ids()
                .iter()
                .any(|id| *id == (
                    flood_request.initiator_id.to_string() + "_" + flood_request.flood_id.to_string().as_str()
                ));
            
            // b. Check if the node has a neighbour, excluding the one from which it received the flood request
            
            // Check if the updated neighbours list is empty
            
            // If I have only one neighbour, I must have received this message from it and I don't have anybody else to forward it to
            let has_no_neighbour: bool = self.get_packet_send().len() == 1;
            
            // 2. Check if the flood request should be sent back as a flood response or broadcasted as is
            if flood_request_is_already_received || has_no_neighbour {
                // A flood response should be created and sent
                
                // a. Create a built response based on the flood request
                let flood_response_packet =
                    self.build_flood_response(packet, flood_request.path_trace);
                
                // Forward the flood response packet
                self.forward_packet(flood_response_packet);
            } else {
                // The packet should be broadcast
                self.get_seen_flood_ids().insert(
                    flood_request.initiator_id.to_string() + "_" + flood_request.flood_id.to_string().as_str()
                );
                
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
    
    /// Builds a flood response packet from a flood request packet and the provided path trace.
    ///
    /// The function reverses the path trace to generate a routing header that guides the
    /// response back to the originator.
    ///
    /// # Arguments
    ///
    /// * `packet` - The original flood request packet.
    /// * `path_trace` - A vector of tuples containing node IDs and their types that represent the path.
    ///
    /// # Returns
    ///
    /// A new packet representing the flood response.
    ///
    /// # Panics
    ///
    /// Panics if the packet is not a flood request.
    fn build_flood_response(
        &mut self,
        packet: Packet,
        path_trace: Vec<(NodeId, NodeType)>,
    ) -> Packet {
        if let PacketType::FloodRequest(flood_request) = packet.pack_type {
            let mut route_back: Vec<NodeId> = path_trace.iter().map(|tuple| tuple.0).collect();
            route_back.reverse(); // Reverse the route for sending back the response
            
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
                session_id: flood_request.flood_id,
            }
        } else {
            panic!("Error! Attempt to build flood response from non-flood request packet");
        }
    }
    
    /// Broadcasts a packet to all neighbouring nodes except the one from which the packet was received.
    ///
    /// For each eligible neighbour, the function updates the routing header to reflect the direct path
    /// from the current node to that neighbour and sends a simulation event.
    ///
    /// # Arguments
    ///
    /// * `packet` - The packet to broadcast.
    /// * `who_i_received_the_packet_from` - The node ID from which the original packet was received.
    fn broadcast_packet(&mut self, packet: Packet, who_i_received_the_packet_from: NodeId) {
        // Copy the list of neighbours and remove the neighbour drone that sent the flood request
        let neighbours: HashMap<NodeId, Sender<Packet>> = self
            .get_packet_send()
            .iter()
            .filter(|(&key, _)| key != who_i_received_the_packet_from)
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        
        // Iterate on the neighbours list
        for (&node_id, sender) in neighbours.iter() {
            let mut packet_to_send = packet.clone();
            packet_to_send.routing_header = SourceRoutingHeader {
                hop_index: 1,
                hops: vec![self.get_id(), node_id],
            };
            // Send a clone of the packet and a simulation event
            if let Err(e) = self
                .get_sim_contr_send()
                .send(DroneEvent::PacketSent(packet_to_send.clone()))
            {
                log_error!(self.get_id(), "Failed to send PacketSent event: {:?}", e);
            }
            if let Err(e) = sender.send(packet_to_send) {
                println!("Failed to send packet to NodeId {:?}: {:?}", node_id, e);
            }
        }
    }
    
    /// Reverses the routing direction of the provided packet.
    ///
    /// This is achieved by removing any nodes beyond the current hop in the routing header,
    /// reversing the order of the hops, and updating the header so the packet can be sent back.
    ///
    /// # Arguments
    ///
    /// * `packet` - The packet whose routing header is to be reversed.
    fn reverse_packet_routing_direction(&self, packet: &mut Packet) {
        // a. Create the route back using the current hops
        let mut hops_vec: Vec<NodeId> = packet.routing_header.hops.clone();
        
        // Remove nodes that should no longer receive the packet
        hops_vec.drain(packet.routing_header.hop_index + 1..=hops_vec.len() - 1);
        
        // Reverse the order to set up the return path
        hops_vec.reverse();
        
        let route_back: SourceRoutingHeader = SourceRoutingHeader {
            hop_index: 1, // Start from the first hop
            hops: hops_vec,
        };
        
        // b. Update the packet's routing header
        packet.routing_header = route_back;
    }
    
    /// Adds a communication channel for a neighbouring node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node ID of the neighbour.
    /// * `sender` - The sender channel associated with the neighbour.
    fn add_channel(&mut self, id: NodeId, sender: Sender<Packet>) {
        let packet_send = self.get_packet_send();
        packet_send.insert(id, sender);
    }
    
    /// Removes the communication channel associated with a neighbouring node.
    ///
    /// Logs an error if no such channel exists.
    ///
    /// # Arguments
    ///
    /// * `id` - The node ID of the neighbour to remove.
    fn remove_channel(&mut self, id: NodeId) {
        if !self.get_packet_send().contains_key(&id) {
            log_error!(
                self.get_id(),
                "Error! The current node {} has no neighbour node {}.",
                self.get_id(),
                id
            );
            return;
        }
        self.get_packet_send().remove(&id);
    }
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
        seen_flood_ids: HashSet<String>,
        senders: HashMap<NodeId, Sender<Packet>>,
        receiver: Receiver<Packet>,
        rng: StdRng,
        sim_controller: Sender<DroneEvent>,
    }
    
    impl NetworkNode for TestNode {
        /// Returns the unique identifier for this test node.
        fn get_id(&self) -> NodeId {
            self.id
        }
        
        /// Provides mutable access to the set of flood request IDs seen by this test node.
        fn get_seen_flood_ids(&mut self) -> &mut HashSet<String> {
            &mut self.seen_flood_ids
        }
        
        /// Returns a mutable reference to the mapping of neighbour nodes to their sender channels.
        fn get_packet_send(&mut self) -> &mut HashMap<NodeId, Sender<Packet>> {
            &mut self.senders
        }
        
        /// Retrieves a reference to the receiver channel for incoming packets.
        fn get_packet_receiver(&self) -> &Receiver<Packet> {
            &self.receiver
        }
        
        /// Returns a mutable reference to the test node's random number generator.
        fn get_random_generator(&mut self) -> &mut StdRng {
            &mut self.rng
        }
        
        /// Returns a reference to the simulation controller sender channel.
        fn get_sim_contr_send(&self) -> &Sender<DroneEvent> {
            &self.sim_controller
        }
        
        /// Test implementation for handling a routed packet.
        /// This function is unimplemented in the test node.
        fn handle_routed_packet(&mut self, _packet: Packet) -> bool {
            unimplemented!()
        }
        
        /// Test implementation for handling a command.
        /// This function is unimplemented in the test node.
        fn handle_command(&mut self, _command: Command) {
            unimplemented!()
        }
    }
    
    impl TestNode {
        /// Creates a new test node with the specified identifier.
        fn new(id: NodeId) -> Self {
            Self {
                id,
                seen_flood_ids: HashSet::new(),
                senders: HashMap::new(),
                receiver: unbounded().1,
                rng: StdRng::from_entropy(),
                sim_controller: unbounded().0,
            }
        }
    }
    
    /// Tests the `forward_packet` function to ensure that a packet is correctly forwarded to the next hop
    /// and that a simulation event is generated.
    #[test]
    fn test_forward_packet() {
        // Create a test node with ID 1
        let mut node = TestNode::new(1);
        
        // Set up communication channel to node 2
        let (sender, receiver) = unbounded();
        let (sim_sender, sim_receiver) = unbounded();
        node.senders.insert(2, sender);
        node.sim_controller = sim_sender;
        
        // Create a test packet with routing header [1, 2] where the current hop is at index 0.
        let packet = Packet {
            pack_type: wg_2024::packet::PacketType::Ack(wg_2024::packet::Ack { fragment_index: 0 }),
            routing_header: wg_2024::network::SourceRoutingHeader {
                hop_index: 1,
                hops: vec![1, 2],
            },
            session_id: 42,
        };
        
        // Test forwarding the packet from node 1 to node 2
        node.forward_packet(packet.clone());
        
        // Verify the packet was received by node 2
        let received = receiver.try_recv().expect("Failed to receive packet");
        assert_eq!(received.session_id, 42);
        
        // Verify that a simulation event was generated
        let sim_event = sim_receiver.try_recv().expect("Failed to receive simulation controller event");
        match sim_event {
            DroneEvent::PacketSent(p) => assert_eq!(p.session_id, 42),
            _ => panic!("Expected PacketSent event"),
        }
    }
}
