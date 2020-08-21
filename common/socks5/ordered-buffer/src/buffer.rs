use crate::message::Message;

/// Stores messages and emits them in order.
///
/// Only contiguous messages with an index less than or equal to `next_index`
/// will be returned - this avoids returning gaps while we wait for the buffer
/// to fill up with the full sequence.
#[derive(Debug)]
pub struct OrderedMessageBuffer {
    next_index: u64,
    messages: Vec<Message>,
}

impl OrderedMessageBuffer {
    pub fn new() -> OrderedMessageBuffer {
        OrderedMessageBuffer {
            next_index: 0,
            messages: Vec::new(),
        }
    }

    /// Writes a message to the buffer. messages are sort on insertion, so
    /// that later on multiple reads for incomplete sequences don't result in
    /// useless sort work.
    pub fn write(&mut self, message: Message) {
        self.messages.push(message);
        OrderedMessageBuffer::insertion_sort(&mut self.messages);
    }

    /// Returns `Option<Vec<u8>>` where it's `Some(bytes)` if there is gapless
    /// ordered data in the buffer, and `None` if the buffer is empty or has
    /// gaps in the contained data. E.g. if the buffer contains message
    /// messages 0, 1, 2, and 4, then a read will return the bytes of messages
    /// 0, 1, 2. Subsequent reads will return `None` until message 3 comes in,
    /// at which point 3, 4, and any further contiguous messages which have arrived
    /// will be returned.
    pub fn read(&mut self) -> Option<Vec<u8>> {
        if self.messages.is_empty() || self.messages.first().unwrap().index > self.next_index {
            return None;
        } else {
            let index = self.next_index.clone() + 1;
            let contiguous_messages: Vec<Message> = self
                .messages
                .iter()
                .filter(|message| message.index <= index)
                .cloned()
                .collect();

            // get rid of all messages we're about to send out of the buffer
            self.messages.retain(|message| message.index > index);

            // advance the index because we've read stuff up to a new high water mark
            let high_water = index + contiguous_messages.len() as u64 - 1;
            self.next_index = high_water;

            // dig out the bytes from inside the struct
            let data = contiguous_messages
                .iter()
                .flat_map(|message| message.data.clone())
                .collect();

            Some(data)
        }
    }

    fn insertion_sort<T>(values: &mut [T])
    where
        T: Ord,
    {
        for i in 0..values.len() {
            for j in (0..i).rev() {
                if values[j] >= values[j + 1] {
                    values.swap(j, j + 1);
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod test_chunking_and_reassembling {
    use super::*;

    #[cfg(test)]
    mod reading_from_and_writing_to_the_buffer {
        use super::*;

        #[cfg(test)]
        mod when_full_ordered_sequence_exists {
            use super::*;
            use crate::message::Message;
            #[test]
            fn read_returns_ordered_bytes_and_resets_buffer() {
                let mut buffer = OrderedMessageBuffer::new();

                let first_message = Message {
                    data: vec![1, 2, 3, 4],
                    index: 0,
                };
                let second_message = Message {
                    data: vec![5, 6, 7, 8],
                    index: 1,
                };

                buffer.write(first_message);
                let first_read = buffer.read().unwrap();
                assert_eq!(vec![1, 2, 3, 4], first_read);

                buffer.write(second_message);
                let second_read = buffer.read().unwrap();
                assert_eq!(vec![5, 6, 7, 8], second_read);

                assert_eq!(None, buffer.read()); // second read on fully ordered result set is empty
            }

            #[test]
            fn test_multiple_adds_stacks_up_bytes_in_the_buffer() {
                let mut buffer = OrderedMessageBuffer::new();

                let first_message = Message {
                    data: vec![1, 2, 3, 4],
                    index: 0,
                };
                let second_message = Message {
                    data: vec![5, 6, 7, 8],
                    index: 1,
                };

                buffer.write(first_message);
                buffer.write(second_message);
                let second_read = buffer.read();
                assert_eq!(vec![1, 2, 3, 4, 5, 6, 7, 8], second_read.unwrap());
                assert_eq!(None, buffer.read()); // second read on fully ordered result set is empty
            }

            #[test]
            fn out_of_order_adds_results_in_ordered_byte_vector() {
                let mut buffer = OrderedMessageBuffer::new();

                let first_message = Message {
                    data: vec![1, 2, 3, 4],
                    index: 0,
                };
                let second_message = Message {
                    data: vec![5, 6, 7, 8],
                    index: 1,
                };

                buffer.write(second_message);
                buffer.write(first_message);
                let read = buffer.read();
                assert_eq!(vec![1, 2, 3, 4, 5, 6, 7, 8], read.unwrap());
                assert_eq!(None, buffer.read()); // second read on fully ordered result set is empty
            }
        }

        mod when_there_are_gaps_in_the_sequence {

            use super::*;
            #[cfg(test)]
            use crate::message::Message;
            fn setup() -> OrderedMessageBuffer {
                let mut buffer = OrderedMessageBuffer::new();

                let zero_message = Message {
                    data: vec![0, 0, 0, 0],
                    index: 0,
                };
                let one_message = Message {
                    data: vec![1, 1, 1, 1],
                    index: 1,
                };

                let three_message = Message {
                    data: vec![3, 3, 3, 3],
                    index: 3,
                };

                buffer.write(zero_message);
                buffer.write(one_message);
                buffer.write(three_message);
                buffer
            }
            #[test]
            fn everything_up_to_the_indexing_gap_is_returned_xxx() {
                let mut buffer = setup();
                let ordered_bytes = buffer.read().unwrap();
                assert_eq!([0, 0, 0, 0, 1, 1, 1, 1].to_vec(), ordered_bytes);

                // we shouldn't get any more from a second attempt if nothing is added
                assert_eq!(None, buffer.read());

                // let's add another message, leaving a gap in place at index 2
                let five_message = Message {
                    data: vec![5, 5, 5, 5],
                    index: 5,
                };
                buffer.write(five_message);
                assert_eq!(None, buffer.read());
            }

            #[test]
            fn filling_the_gap_allows_us_to_get_everything() {
                let mut buffer = setup();
                buffer.read(); // that burns the first two. We still have a gap before the 3s.

                let two_message = Message {
                    data: vec![2, 2, 2, 2],
                    index: 2,
                };
                buffer.write(two_message);

                let more_ordered_bytes = buffer.read().unwrap();
                assert_eq!([2, 2, 2, 2, 3, 3, 3, 3].to_vec(), more_ordered_bytes);

                // let's add another message
                let five_message = Message {
                    data: vec![5, 5, 5, 5],
                    index: 5,
                };
                buffer.write(five_message);

                assert_eq!(None, buffer.read());

                // let's fill in the gap of 4s now and read again
                let four_message = Message {
                    data: vec![4, 4, 4, 4],
                    index: 4,
                };
                buffer.write(four_message);

                assert_eq!([4, 4, 4, 4, 5, 5, 5, 5].to_vec(), buffer.read().unwrap());

                // at this point we should again get back nothing if we try a read
                assert_eq!(None, buffer.read());
            }
        }
    }
}
