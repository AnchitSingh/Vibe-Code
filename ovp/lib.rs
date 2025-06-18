use std::os::unix::io::RawFd;
use std::ptr;
use std::mem;
use std::ffi::CString;
use std::sync::Arc;
use std::thread;
use std::sync::mpsc::{channel, Receiver, Sender};

// Raw socket constants
const AF_PACKET: i32 = 17;
const SOCK_RAW: i32 = 3;
const ETH_P_ALL: u16 = 0x0003;
const SOL_PACKET: i32 = 263;
const PACKET_ADD_MEMBERSHIP: i32 = 1;
const PACKET_MR_PROMISC: i32 = 1;

// OVP Protocol Magic
const OVP_MAGIC: u32 = 0xDEADBEEF;

// Core types
pub type DroneId = u64;

#[repr(C, packed)]
#[derive(Debug)]
pub struct OmegaFrame {
    magic: u32,
    target_count: u32,
    payload_len: u32,
    // Dynamic data follows: targets[], payload[]
}

#[repr(C)]
struct PacketMreq {
    mr_ifindex: i32,
    mr_type: u16,
    mr_alen: u16,
    mr_address: [u8; 8],
}

#[repr(C)]
struct SockaddrLl {
    sll_family: u16,
    sll_protocol: u16,
    sll_ifindex: i32,
    sll_hatype: u16,
    sll_pkttype: u8,
    sll_halen: u8,
    sll_addr: [u8; 8],
}

unsafe extern "C" {
    unsafe fn socket(domain: i32, type_: i32, protocol: i32) -> i32;
    unsafe fn setsockopt(sockfd: i32, level: i32, optname: i32, optval: *const u8, optlen: u32) -> i32;
    unsafe fn bind(sockfd: i32, addr: *const SockaddrLl, addrlen: u32) -> i32;
    unsafe fn sendto(sockfd: i32, buf: *const u8, len: usize, flags: i32, 
              dest_addr: *const SockaddrLl, addrlen: u32) -> isize;
    unsafe fn recvfrom(sockfd: i32, buf: *mut u8, len: usize, flags: i32,
                src_addr: *mut SockaddrLl, addrlen: *mut u32) -> isize;
    unsafe fn if_nametoindex(ifname: *const i8) -> u32;
    unsafe fn close(fd: i32) -> i32;
}

pub struct OmegaSocket {
    raw_fd: RawFd,
    interface_index: u32,
}

impl OmegaSocket {
    pub fn new(interface: &str) -> Result<Self, Box<dyn std::error::Error>> {
        unsafe {
            // Create raw packet socket
            let fd = socket(AF_PACKET, SOCK_RAW, (ETH_P_ALL as u16).to_be() as i32);
            if fd < 0 {
                return Err("Failed to create raw socket".into());
            }

            // Get interface index
            let if_name = CString::new(interface)?;
            let if_index = if_nametoindex(if_name.as_ptr());
            if if_index == 0 {
                close(fd);
                return Err("Interface not found".into());
            }

            // Enable promiscuous mode - RECEIVE EVERYTHING
            let mreq = PacketMreq {
                mr_ifindex: if_index as i32,
                mr_type: PACKET_MR_PROMISC as u16,
                mr_alen: 0,
                mr_address: [0; 8],
            };

            let ret = setsockopt(
                fd,
                SOL_PACKET,
                PACKET_ADD_MEMBERSHIP,
                &mreq as *const _ as *const u8,
                mem::size_of::<PacketMreq>() as u32,
            );

            if ret < 0 {
                close(fd);
                return Err("Failed to set promiscuous mode".into());
            }

            // Bind to interface
            let addr = SockaddrLl {
                sll_family: AF_PACKET as u16,
                sll_protocol: (ETH_P_ALL as u16).to_be(),
                sll_ifindex: if_index as i32,
                sll_hatype: 0,
                sll_pkttype: 0,
                sll_halen: 0,
                sll_addr: [0; 8],
            };

            let ret = bind(fd, &addr, mem::size_of::<SockaddrLl>() as u32);
            if ret < 0 {
                close(fd);
                return Err("Failed to bind socket".into());
            }

            Ok(OmegaSocket {
                raw_fd: fd,
                interface_index: if_index,
            })
        }
    }

    // THE MAGIC: One spherical emit to rule them all
    pub fn emit_frame(&self, frame_data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            let dest_addr = SockaddrLl {
                sll_family: AF_PACKET as u16,
                sll_protocol: (ETH_P_ALL as u16).to_be(),
                sll_ifindex: self.interface_index as i32,
                sll_hatype: 0,
                sll_pkttype: 0,
                sll_halen: 6, // MAC address length
                sll_addr: [0xFF; 8], // Broadcast MAC
            };

            let sent = sendto(
                self.raw_fd,
                frame_data.as_ptr(),
                frame_data.len(),
                0,
                &dest_addr,
                mem::size_of::<SockaddrLl>() as u32,
            );

            if sent < 0 {
                return Err("Failed to send frame".into());
            }

            Ok(())
        }
    }

    // Receive ANY frame in range
    pub fn receive_frame(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        unsafe {
            let mut buffer = vec![0u8; 65536]; // Max ethernet frame
            let mut src_addr: SockaddrLl = mem::zeroed();
            let mut addr_len = mem::size_of::<SockaddrLl>() as u32;

            let received = recvfrom(
                self.raw_fd,
                buffer.as_mut_ptr(),
                buffer.len(),
                0,
                &mut src_addr,
                &mut addr_len,
            );

            if received < 0 {
                return Err("Failed to receive frame".into());
            }

            buffer.truncate(received as usize);
            Ok(buffer)
        }
    }
}

impl Drop for OmegaSocket {
    fn drop(&mut self) {
        unsafe {
            close(self.raw_fd);
        }
    }
}

// THE CLIENT API - Simple as fuck
pub struct OVP {
    socket: Arc<OmegaSocket>,
    my_drone_id: DroneId,
    receiver_handle: Option<thread::JoinHandle<()>>,
    message_rx: Receiver<Vec<u8>>,
}

impl Clone for OVP {
    fn clone(&self) -> Self {
        // Create a new OVP instance with the same socket and drone ID
        // Note: This creates a new receiver channel since we can't clone the existing one
        let (tx, rx) = std::sync::mpsc::channel();
        
        // Create a new OVP instance with the cloned socket and new receiver
        OVP {
            socket: self.socket.clone(),
            my_drone_id: self.my_drone_id,
            receiver_handle: None, // New instance won't have a receiver handle
            message_rx: rx,       // New receiver channel
        }
    }
}

impl OVP {
    pub fn new(interface: &str, my_drone_id: DroneId) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = Arc::new(OmegaSocket::new(interface)?);
        let (tx, rx) = channel();
        
        // Start background receiver thread
        let socket_clone = Arc::clone(&socket);
        let handle = thread::spawn(move || {
            loop {
                match socket_clone.receive_frame() {
                    Ok(frame) => {
                        if let Some(payload) = parse_ovp_frame(&frame, my_drone_id) {
                            let _ = tx.send(payload);
                        }
                    }
                    Err(_) => continue,
                }
            }
        });

        Ok(OVP {
            socket,
            my_drone_id,
            receiver_handle: Some(handle),
            message_rx: rx,
        })
    }

    // THE ONLY API METHOD - Pure volumetric power
    pub fn emit(&self, neighbours: Option<Vec<DroneId>>, payload: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        let targets = neighbours.unwrap_or_else(Vec::new);
        let frame = build_ovp_frame(&targets, &payload);
        
        // ONE SPHERICAL EMIT - REACHES ALL IN RANGE
        self.socket.emit_frame(&frame)?;
        
        Ok(())
    }

    // Non-blocking message receive
    pub fn try_receive(&self) -> Option<Vec<u8>> {
        self.message_rx.try_recv().ok()
    }
}

// Frame building/parsing helpers
fn build_ovp_frame(targets: &[DroneId], payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    
    // Header
    frame.extend_from_slice(&OVP_MAGIC.to_le_bytes());
    frame.extend_from_slice(&(targets.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    
    // Targets
    for &target in targets {
        frame.extend_from_slice(&target.to_le_bytes());
    }
    
    // Payload
    frame.extend_from_slice(payload);
    
    frame
}

fn parse_ovp_frame(frame: &[u8], my_id: DroneId) -> Option<Vec<u8>> {
    if frame.len() < 12 { return None; }
    
    // Check magic
    let magic = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
    if magic != OVP_MAGIC { return None; }
    
    let target_count = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    let payload_len = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
    
    let targets_start = 12;
    let targets_end = targets_start + (target_count * 8);
    let payload_start = targets_end;
    let payload_end = payload_start + payload_len;
    
    if frame.len() < payload_end { return None; }
    
    // Check if message is for me
    if target_count > 0 {
        let mut found = false;
        for i in 0..target_count {
            let offset = targets_start + (i * 8);
            let target_id = u64::from_le_bytes([
                frame[offset], frame[offset+1], frame[offset+2], frame[offset+3],
                frame[offset+4], frame[offset+5], frame[offset+6], frame[offset+7]
            ]);
            if target_id == my_id {
                found = true;
                break;
            }
        }
        if !found { return None; }
    }
    
    // Extract payload
    Some(frame[payload_start..payload_end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_building() {
        let targets = vec![1, 2, 3];
        let payload = b"Hello volumetric world!";
        let frame = build_ovp_frame(&targets, payload);
        
        assert!(frame.len() > 12);
        
        let parsed = parse_ovp_frame(&frame, 2);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap(), payload.to_vec());
    }
}