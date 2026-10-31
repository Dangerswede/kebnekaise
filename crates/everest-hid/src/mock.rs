//! A scripted [`Transport`] for tests: no hardware involved.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use everest_proto::PACKET_SIZE;

use crate::transport::Transport;
use crate::Result;

/// Answers each written command with the packets registered for it.
#[derive(Default)]
pub struct MockTransport {
    responses: HashMap<[u8; 2], VecDeque<Vec<[u8; PACKET_SIZE]>>>,
    sticky: HashMap<[u8; 2], Vec<[u8; PACKET_SIZE]>>,
    inbox: VecDeque<[u8; PACKET_SIZE]>,
    written: Vec<[u8; PACKET_SIZE]>,
}

impl MockTransport {
    /// The next write of `command` queues `packets` for reading (once).
    pub fn on(&mut self, command: [u8; 2], packets: Vec<[u8; PACKET_SIZE]>) {
        self.responses
            .entry(command)
            .or_default()
            .push_back(packets);
    }

    /// Every write of `command` queues `packets`, after any one-shot answers.
    pub fn always(&mut self, command: [u8; 2], packets: Vec<[u8; PACKET_SIZE]>) {
        self.sticky.insert(command, packets);
    }

    /// Queues a packet as if the device had sent it unprompted.
    pub fn push_unsolicited(&mut self, packet: [u8; PACKET_SIZE]) {
        self.inbox.push_back(packet);
    }

    /// Everything written so far, in order.
    pub fn written(&self) -> &[[u8; PACKET_SIZE]] {
        &self.written
    }
}

impl Transport for MockTransport {
    fn write_raw(&mut self, packet: &[u8; PACKET_SIZE]) -> Result<()> {
        self.written.push(*packet);
        let command = [packet[0], packet[1]];
        let answer = match self
            .responses
            .get_mut(&command)
            .and_then(VecDeque::pop_front)
        {
            Some(a) => Some(a),
            None => self.sticky.get(&command).cloned(),
        };
        self.inbox.extend(answer.into_iter().flatten());
        Ok(())
    }

    fn read_raw(&mut self, _timeout: Duration) -> Result<Option<[u8; PACKET_SIZE]>> {
        Ok(self.inbox.pop_front())
    }
}
