use crate::{task::executor::yield_once, virtio::Virtio};

#[repr(C)]
#[derive(Debug)]
struct VirtioInputEvent {
    type_: u16,
    code: u16,
    value: u32,
}
///Handle the virtio device and export all data to globals::Input
pub async fn drive(mut virtio: Virtio) {
    unsafe {
        let q = 0;
        virtio.queue_select(q);
        while let Some(desc_id) = virtio.get_free_desc_id() {
            virtio.set_writable_available(desc_id);
        }
        loop {
            while let Some(used) = virtio.next_used() {
                let desc = virtio.read_desc(used.id as u16);
                let evt = (desc.addr as *const VirtioInputEvent).read_volatile();
                crate::globals::INPUT.update(|input| match evt.type_ {
                    0 => { /*no op */ }
                    1 => input.keys[evt.code as usize].handle_incoming_state(evt.value != 0),
                    2 => {
                        let d: i32 = core::intrinsics::transmute(evt.value);
                        match evt.code {
                            0 => input.mouse_x = (input.mouse_x as i32 + d).max(0) as usize,
                            1 => input.mouse_y = (input.mouse_y as i32 + d).max(0) as usize,
                            _ => log::error!("virtio_input: unknown event {:?}", evt),
                        }
                    }
                    _ => log::error!("virtio_input: unknown event {:?}", evt),
                });
                virtio.set_writable_available(used.id as u16);
            }
            yield_once().await;
        }
    }
}
