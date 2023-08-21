use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::{read_volatile, write_volatile},
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{sync::Arc, vec::Vec};
use futures::task::AtomicWaker;
use lazy_static::lazy_static;
use x86_64::{
    structures::paging::{frame, FrameAllocator, Mapper, Size4KiB},
    VirtAddr,
};

use crate::{
    allocator::{self, ALLOCATOR},
    create_identity_virt_from_phys_n,
    framebuffer::{FB, RGBA},
    interrupts::global_time_ms,
    task::{
        executor::{yield_once, Spawner},
        Task,
    },
    virtio::{Desc, Virtio},
};
use spin::Mutex;
#[derive(Debug)]
pub enum IdWaker {
    None,
    Done,
    Waker(AtomicWaker),
}

impl IdWaker {
    pub fn wake(&mut self) {
        let old = core::mem::replace(self, Self::Done);
        if let IdWaker::Waker(e) = old {
            e.wake();
        } else {
            log::error!("Nothing to wake");
        }
    }
}

lazy_static! {
    pub static ref WAKERS: Mutex<[IdWaker; 256]> = Mutex::new([(); 256].map(|_| IdWaker::None));
}

pub async fn drive(mut virtio: Virtio, spawner: Spawner, fb: *mut FB) {
    unsafe {
        let q = 0;
        virtio.queue_select(q);
        let queue = read_volatile(virtio.common.cap);

        let virtio = Arc::new(Mutex::new(virtio));

        let virtio_2 = Arc::clone(&virtio);
        spawner.run(async move {
            loop {
                'checkall: loop {
                    let next = { virtio_2.lock().next_used() };
                    if let Some(used) = next {
                        WAKERS.lock()[used.id as usize].wake();
                    } else {
                        break 'checkall;
                    }
                }
                yield_once().await;
            }
        });

        //Make a few free desc
        {
            let mut virtio = virtio.lock();
            for _ in 0..10 {
                if let Some(desc_id) = virtio.get_free_desc_id() {
                    virtio.set_writable_available(desc_id);
                }
            }
            yield_once().await;
        }

        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCtrlHdr {
                type_: VirtioGpuCtrlType::VirtioGpuCmdGetDisplayInfo,
                ..Default::default()
            },
        )
        .await;
        let mut display_info =
            (response_desc.addr as *const VirtioGpuRespOkDisplayInfo).read_volatile();
        log::info!("{:?}", display_info);

        {
            #[repr(C)]
            #[derive(Clone, Debug)]
            struct VirtioGpuConfig {
                events_read: u32,
                events_clear: u32,
                num_scanouts: u32,
                num_capsets: u32,
            }
            let conf_ptr: *mut VirtioGpuConfig =
                core::intrinsics::transmute((virtio.lock().device.cap) as *const ());
            let mut rconf = conf_ptr.read_volatile();

            for i in 0..rconf.num_capsets {
                let response_desc = request(
                    Arc::clone(&virtio),
                    VirtioGpuCmdGetCapsetInfo {
                        header: VirtioGpuCtrlHdr {
                            type_: VirtioGpuCtrlType::VirtioGpuCmdGetCapsetInfo,
                            ..Default::default()
                        },
                        capset_index: i as u32,
                        padding: 0,
                    },
                )
                .await;
                let capsetinfo =
                    (response_desc.addr as *const VirtioGpuRespCapsetInfo).read_volatile();
                log::info!("CAP {}, {:?}", i, capsetinfo);
            }
            yield_once().await;
        }

        // for capn in 0..display_info.pmodes.

        display_info.pmodes.rect.w = 1600;
        display_info.pmodes.rect.h = 900;

        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCtrlHdr {
                type_: VirtioGpuCtrlType::VirtioGpuCmdGetEdid,
                ..Default::default()
            },
        )
        .await;
        let edid = (response_desc.addr as *const VirtioGpuRespEdid).read_volatile();
        // log::info!("{:?}", edid);

        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCmdResourceCreate2d {
                header: VirtioGpuCtrlHdr {
                    type_: VirtioGpuCtrlType::VirtioGpuCmdResourceCreate2d,
                    ..Default::default()
                },
                resource_id: 1,
                format: VirtioGpuFormats::VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM,
                width: display_info.pmodes.rect.w,
                height: display_info.pmodes.rect.h,
            },
        )
        .await;
        let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
        log::info!("{:?}", nodata.type_);

        let capacity = (display_info.pmodes.rect.w * display_info.pmodes.rect.h) as usize;

        let framebuffer_bytes = capacity * 4;
        let pages_needed = 1 + framebuffer_bytes / 4096;
        let pages = create_identity_virt_from_phys_n(pages_needed).unwrap();
        let addr = pages.start_address().as_u64();
        // let framebuffer_ptr =
        //     ALLOCATOR.alloc(Layout::from_size_align_unchecked(capacity * 4, 4096));
        let mut framebuffer: Vec<RGBA> = Vec::from_raw_parts(addr as *mut RGBA, capacity, capacity);
        log::info!("(*fb).update {:?}", addr as *mut RGBA);
        (*fb).update(
            addr as *mut RGBA,
            display_info.pmodes.rect.w as usize,
            display_info.pmodes.rect.h as usize,
        );

        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCmdResourceAttachBacking {
                header: VirtioGpuCtrlHdr {
                    type_: VirtioGpuCtrlType::VirtioGpuCmdResourceAttachBacking,
                    ..Default::default()
                },
                resource_id: 1,
                nr_entries: 1,
                //mem
                addr,
                length: (core::mem::size_of::<RGBA>() * framebuffer.len()) as u32,
                padding: 0,
            },
        )
        .await;
        let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
        log::info!("{:?}", nodata.type_);

        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCmdSetScanout {
                header: VirtioGpuCtrlHdr {
                    type_: VirtioGpuCtrlType::VirtioGpuCmdSetScanout,
                    ..Default::default()
                },
                r: display_info.pmodes.rect,
                resource_id: 1,
                scanout_id: 0,
            },
        )
        .await;
        let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
        log::info!("{:?}", nodata.type_);

        for i in 0..capacity {
            framebuffer[i] = (RGBA {
                r: 100,
                g: 120,
                b: 140,
                a: 125,
            });
        }

        //FIRST TRANSFER AND FLUSH
        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCmdTransferToHost2d {
                header: VirtioGpuCtrlHdr {
                    type_: VirtioGpuCtrlType::VirtioGpuCmdTransferToHost2d,
                    ..Default::default()
                },
                r: display_info.pmodes.rect,
                resource_id: 1,
                padding: 0,
                offset: 0,
            },
        )
        .await;
        let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
        log::info!("{:?}", nodata.type_);

        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCmdResourceFlush {
                header: VirtioGpuCtrlHdr {
                    type_: VirtioGpuCtrlType::VirtioGpuCmdResourceFlush,
                    ..Default::default()
                },
                r: display_info.pmodes.rect,
                resource_id: 1,
                padding: 0,
            },
        )
        .await;
        let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
        log::info!("{:?}", nodata.type_);

        let mut debug_name: [char; 64] = ['1'; 64];
        let name = "Debug\0";
        for (index, e) in name.chars().enumerate() {
            debug_name[index] = e;
        }
        let response_desc = request(
            Arc::clone(&virtio),
            VirtioGpuCmdCtxCreate {
                header: VirtioGpuCtrlHdr {
                    type_: VirtioGpuCtrlType::VirtioGpuCmdCtxCreate,
                    ctx_id: 1,
                    ..Default::default()
                },
                nlen: name.len() as u32,
                debug_name,
                context_init: 0,
            },
        )
        .await;
        let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
        log::info!("VirtioGpuCmdCtxCreate {:?}", nodata.type_);

        //FIRST 3D SUBMIT
        {
            let mut buffer: Vec<u32> = Vec::with_capacity(512);

            fn cmd_clear(
                buffer: &mut Vec<u32>,
                buffers: u32,
                rgba: [u8; 4],
                depth: f64,
                stencil: u32,
            ) {
                let len = 8;
                buffer.push((len << 16) + (0 << 8) + Cmd3d::VIRGL_CCMD_CLEAR as u32);
                //Buffer select
                buffer.push(buffers);
                buffer.push(rgba[0] as u32);
                buffer.push(rgba[1] as u32);
                buffer.push(rgba[2] as u32);
                buffer.push(rgba[3] as u32);
                //Depth
                buffer.push(0);
                buffer.push(0);
                //stencil
                buffer.push(0);
            }

            fn cmd_set_framebuffer_state(buffer: &mut Vec<u32>, handles: &[u32]) {
                let len = handles.len() as u32 + 2;
                buffer
                    .push((len << 16) + (0 << 8) + Cmd3d::VIRGL_CCMD_SET_FRAMEBUFFER_STATE as u32);
                //Buffer select
                buffer.push(handles.len() as u32);
                buffer.push(0);
                for handle in handles.iter() {
                    buffer.push(*handle);
                }
            }

            fn cmd_create_surface(buffer: &mut Vec<u32>, handle: u32, format: VirglFormats) {
                let len = 5;
                buffer.push(
                    (len << 16)
                        | ((VirglObjectType::VIRGL_OBJECT_SURFACE as u32) << 8)
                        | Cmd3d::VIRGL_CCMD_CREATE_OBJECT as u32,
                );
                //Buffer select
                buffer.push(handle);
                buffer.push(handle);
                buffer.push(format as u32);
                buffer.push(0);
                buffer.push(0);
            }

            let res_handle = 2;

            let mut args = VirglRendererResourceCreateArgs::default();
            args.width = 256;
            args.height = 256;
            args.handle = res_handle;
            // args.target = PipeTextureTarget::PIPE_BUFFER;
            args.bind = PIPE_BIND_SAMPLER_VIEW;

            let response_desc = request(
                Arc::clone(&virtio),
                VirtioGpuCmdResourceCreate3d {
                    header: VirtioGpuCtrlHdr {
                        type_: VirtioGpuCtrlType::VirtioGpuCmdResourceCreate3d,
                        ctx_id: 1,
                        ..Default::default()
                    },
                    args,
                    padding: 0,
                },
            )
            .await;
            let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
            log::info!("VirtioGpuCmdResourceCreate3d {:?}", nodata.type_);

            let response_desc = request(
                Arc::clone(&virtio),
                VirtioGpuCmdCtxAttachResource {
                    header: VirtioGpuCtrlHdr {
                        type_: VirtioGpuCtrlType::VirtioGpuCmdCtxAttachResource,
                        ctx_id: 1,
                        ..Default::default()
                    },
                    handle: res_handle,
                    padding: 0,
                },
            )
            .await;
            let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();

            log::info!("VirtioGpuCmdCtxAttachResource {:?}", nodata.type_);

            let resource_example = {
                let capacity = (256 * 256) as usize;
                let framebuffer_bytes = capacity * 4;
                let pages_needed = 1 + framebuffer_bytes / 4096;
                let pages = create_identity_virt_from_phys_n(pages_needed).unwrap();
                let addr = pages.start_address().as_u64();
                let mut framebuffer: Vec<RGBA> =
                    Vec::from_raw_parts(addr as *mut RGBA, capacity, capacity);
                framebuffer
            };

            let response_desc = request(
                Arc::clone(&virtio),
                VirtioGpuCmdResourceAttachBacking {
                    header: VirtioGpuCtrlHdr {
                        type_: VirtioGpuCtrlType::VirtioGpuCmdResourceAttachBacking,
                        ctx_id: 1,
                        ..Default::default()
                    },
                    resource_id: res_handle,
                    nr_entries: 1,
                    //mem
                    addr: resource_example.as_ptr() as u64,
                    length: (core::mem::size_of::<RGBA>() * resource_example.len()) as u32,
                    padding: 0,
                },
            )
            .await;
            let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
            log::info!("{:?}", nodata.type_);

            cmd_create_surface(&mut buffer, res_handle, args.format);
            cmd_set_framebuffer_state(&mut buffer, &[res_handle]);
            cmd_clear(&mut buffer, PIPE_CLEAR_COLOR, [255, 0, 0, 255], 0.0, 0);

            let len = buffer.len() as u32;
            // pad to 512
            while buffer.len() < 512 {
                buffer.push(0);
            }
            let buffer: [u32; 512] = buffer.try_into().unwrap();
            let response_desc = request(
                Arc::clone(&virtio),
                VirtioGpuCmdSubmit3d {
                    header: VirtioGpuCtrlHdr {
                        type_: VirtioGpuCtrlType::VirtioGpuCmdSubmit3d,
                        ctx_id: 1,
                        ..Default::default()
                    },
                    len,
                    buffer,
                },
            )
            .await;
            let nodata = (response_desc.addr as *const VirtioGpuCtrlHdr).read_volatile();
            log::info!("VirtioGpuCmdSubmit3d {:?}", nodata.type_);
        }

        let mut b: u8 = 0;

        // spawner.new(async move {
        //     loop {
        //         b = b.wrapping_add(1);
        //         for i in 0..capacity {
        //             framebuffer[i] = (RGBA {
        //                 r: 100,
        //                 g: 120,
        //                 b: b.wrapping_add((i % 256) as u8),
        //                 a: 125,
        //             });
        //         }

        //         yield_once().await;
        //     }
        // });

        if true {
            let virtio_2 = Arc::clone(&virtio);
            spawner.run(async move {
                loop {
                    request(
                        Arc::clone(&virtio_2),
                        VirtioGpuCmdTransferToHost2d {
                            header: VirtioGpuCtrlHdr {
                                type_: VirtioGpuCtrlType::VirtioGpuCmdTransferToHost2d,
                                ..Default::default()
                            },
                            r: display_info.pmodes.rect,
                            resource_id: 1,
                            padding: 0,
                            offset: 0,
                        },
                    )
                    .await;
                }
            });
        }
        loop {
            use futures::join;
            join!(
                // request(
                //     Arc::clone(&virtio),
                //     VirtioGpuCmdTransferToHost2d {
                //         header: VirtioGpuCtrlHdr {
                //             type_: VirtioGpuCtrlType::VirtioGpuCmdTransferToHost2d,
                //             ..Default::default()
                //         },
                //         r: display_info.pmodes.rect,
                //         resource_id: 1,
                //         padding: 0,
                //         offset: 0,
                //     },
                //
                // ),
                request(
                    Arc::clone(&virtio),
                    VirtioGpuCmdResourceFlush {
                        header: VirtioGpuCtrlHdr {
                            type_: VirtioGpuCtrlType::VirtioGpuCmdResourceFlush,
                            ..Default::default()
                        },
                        r: display_info.pmodes.rect,
                        resource_id: 1,
                        padding: 0,
                    },
                )
            );
            let now = global_time_ms();
            let elapsed = now - LAST_FLUSH_MS.load(Ordering::Relaxed);
            LAST_FLUSH_MS.store(now, Ordering::Relaxed);
            // log::info!("gpu start {} elapsed {}", now, elapsed);
            // while elapsed < 10 {
            //     yield_once().await;
            //     elapsed = get_time_ms() - start;
            // }

            // yield_once().await;
        }
    }
}

static LAST_FLUSH_MS: AtomicU64 = AtomicU64::new(0);

pub async fn request<T>(virtio: Arc<Mutex<Virtio>>, data: T) -> Desc {
    let twice = { virtio.lock().get_free_twice_desc_id() };
    if let Some((desc_id, desc_next_id)) = twice {
        {
            virtio.lock().add_request(desc_id, desc_next_id, data);
            virtio.lock().kick(0);
        }
        wait_for(desc_id as usize).await;

        {
            let v = &mut virtio.lock();
            v.set_free_desc_id(desc_id);
            v.set_free_desc_id(desc_next_id);
        }

        unsafe { virtio.lock().read_desc(desc_next_id) }
    } else {
        panic!("No more desc available")
    }
}

pub async fn wait_for(id: usize) {
    IdWait::new(id).await;
}

#[derive(Debug)]
pub struct IdWait {
    id: usize,
}

impl IdWait {
    pub fn new(id: usize) -> Self {
        IdWait { id }
    }
}
impl futures::future::Future for IdWait {
    type Output = ();
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        {
            let val = &mut WAKERS.lock()[self.id];
            match val {
                IdWaker::None => {
                    let aw = AtomicWaker::new();
                    aw.register(&cx.waker());
                    *val = IdWaker::Waker(aw);

                    return core::task::Poll::Pending;
                }
                IdWaker::Done => {
                    *val = IdWaker::None;
                    return core::task::Poll::Ready(());
                }
                IdWaker::Waker(e) => {
                    e.register(&cx.waker());
                }
            }
        }
        {
            let val = &mut WAKERS.lock()[self.id];
            match val {
                IdWaker::Done => {
                    *val = IdWaker::None;
                    core::task::Poll::Ready(())
                }
                _ => core::task::Poll::Pending,
            }
        }
    }
}

#[repr(u32)]
#[derive(Clone, Debug)]
enum VirtioGpuCtrlType {
    /* 2d commands */
    VirtioGpuCmdGetDisplayInfo = 0x0100,
    VirtioGpuCmdResourceCreate2d,
    VirtioGpuCmdResourceUnref,
    VirtioGpuCmdSetScanout,
    VirtioGpuCmdResourceFlush,
    VirtioGpuCmdTransferToHost2d,
    VirtioGpuCmdResourceAttachBacking,
    VirtioGpuCmdResourceDetachBacking,
    VirtioGpuCmdGetCapsetInfo,
    VirtioGpuCmdGetCapset,
    VirtioGpuCmdGetEdid,
    VirtioGpuCmdResourceAssignUuid,
    VirtioGpuCmdResourceCreateBlob,
    VirtioGpuCmdSetScanoutBlob,

    /* 3d commands */
    VirtioGpuCmdCtxCreate = 0x0200,
    VirtioGpuCmdCtxDestroy,
    VirtioGpuCmdCtxAttachResource,
    VirtioGpuCmdCtxDetachResource,
    VirtioGpuCmdResourceCreate3d,
    VirtioGpuCmdTransferToHost3d,
    VirtioGpuCmdTransferFromHost3d,
    VirtioGpuCmdSubmit3d,
    VirtioGpuCmdResourceMapBlob,
    VirtioGpuCmdResourceUnmapBlob,

    /* cursor commands */
    VirtioGpuCmdUpdateCursor = 0x0300,
    VirtioGpuCmdMoveCursor,

    /* success responses */
    VirtioGpuRespOkNoData = 0x1100,
    VirtioGpuRespOkDisplayInfo,
    VirtioGpuRespOkCapsetInfo,
    VirtioGpuRespOkCapset,
    VirtioGpuRespOkEdid,
    VirtioGpuRespOkResourceUuid,
    VirtioGpuRespOkMapInfo,

    /* error responses */
    VirtioGpuRespErrUnspec = 0x1200,
    VirtioGpuRespErrOutOfMemory,
    VirtioGpuRespErrInvalidScanoutId,
    VirtioGpuRespErrInvalidResourceId,
    VirtioGpuRespErrInvalidContextId,
    VirtioGpuRespErrInvalidParameter,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCtrlHdr {
    type_: VirtioGpuCtrlType,
    flags: u32,
    fence_id: u64,
    ctx_id: u32,
    ring_idx: u8,
    padding: [u8; 3],
}

impl Default for VirtioGpuCtrlHdr {
    fn default() -> Self {
        Self {
            type_: VirtioGpuCtrlType::VirtioGpuCmdGetDisplayInfo,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            ring_idx: 0,
            padding: [0, 0, 0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, Copy)]
struct VirtioGpuRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

#[repr(C)]
#[derive(Clone, Debug, Copy)]
struct VirtioGpuDisplay {
    rect: VirtioGpuRect,
    enabled: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuRespOkDisplayInfo {
    header: VirtioGpuCtrlHdr,
    pmodes: VirtioGpuDisplay,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdGetEdid {
    header: VirtioGpuCtrlHdr,
    scanout: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuRespEdid {
    header: VirtioGpuCtrlHdr,
    size: u32,
    padding: u32,
    edid: [u8; 1024],
}
#[repr(u32)]
#[derive(Clone, Debug)]
enum VirtioGpuFormats {
    VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM = 1,
    VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM = 2,
    VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM = 3,
    VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM = 4,
    VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM = 67,
    VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM = 68,
    VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM = 121,
    VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM = 134,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdResourceCreate2d {
    header: VirtioGpuCtrlHdr,
    resource_id: u32,
    format: VirtioGpuFormats,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdResourceAttachBacking {
    header: VirtioGpuCtrlHdr,
    resource_id: u32,
    nr_entries: u32,
    addr: u64,
    length: u32,
    padding: u32,
}
#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdSetScanout {
    header: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdTransferToHost2d {
    header: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}
#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdResourceFlush {
    header: VirtioGpuCtrlHdr,
    r: VirtioGpuRect,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdCtxCreate {
    header: VirtioGpuCtrlHdr,
    nlen: u32,
    context_init: u32,
    debug_name: [char; 64],
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdGetCapsetInfo {
    header: VirtioGpuCtrlHdr,
    capset_index: u32,
    padding: u32,
}

#[repr(u32)]
#[derive(Clone, Debug)]
enum CapsetId {
    VIRTIO_GPU_CAPSET_VIRGL = 1,
    VIRTIO_GPU_CAPSET_VIRGL2 = 2,
    VIRTIO_GPU_CAPSET_GFXSTREAM = 3,
    VIRTIO_GPU_CAPSET_VENUS = 4,
    VIRTIO_GPU_CAPSET_CROSS_DOMAIN = 5,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuRespCapsetInfo {
    header: VirtioGpuCtrlHdr,
    capset_id: CapsetId,
    capset_max_version: u32,
    capset_max_size: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdSubmit3d {
    header: VirtioGpuCtrlHdr,
    len: u32,
    buffer: [u32; 512],
}

#[repr(u32)]
#[derive(Clone, Debug)]
enum Cmd3d {
    VIRGL_CCMD_NOP = 0,
    VIRGL_CCMD_CREATE_OBJECT = 1,
    VIRGL_CCMD_BIND_OBJECT,
    VIRGL_CCMD_DESTROY_OBJECT,
    VIRGL_CCMD_SET_VIEWPORT_STATE,
    VIRGL_CCMD_SET_FRAMEBUFFER_STATE,
    VIRGL_CCMD_SET_VERTEX_BUFFERS,
    VIRGL_CCMD_CLEAR,
    VIRGL_CCMD_DRAW_VBO,
    VIRGL_CCMD_RESOURCE_INLINE_WRITE,
    VIRGL_CCMD_SET_SAMPLER_VIEWS,
    VIRGL_CCMD_SET_INDEX_BUFFER,
    VIRGL_CCMD_SET_CONSTANT_BUFFER,
    VIRGL_CCMD_SET_STENCIL_REF,
    VIRGL_CCMD_SET_BLEND_COLOR,
    VIRGL_CCMD_SET_SCISSOR_STATE,
    VIRGL_CCMD_BLIT,
    VIRGL_CCMD_RESOURCE_COPY_REGION,
    VIRGL_CCMD_BIND_SAMPLER_STATES,
    VIRGL_CCMD_BEGIN_QUERY,
    VIRGL_CCMD_END_QUERY,
    VIRGL_CCMD_GET_QUERY_RESULT,
    VIRGL_CCMD_SET_POLYGON_STIPPLE,
    VIRGL_CCMD_SET_CLIP_STATE,
    VIRGL_CCMD_SET_SAMPLE_MASK,
    VIRGL_CCMD_SET_STREAMOUT_TARGETS,
    VIRGL_CCMD_SET_RENDER_CONDITION,
    VIRGL_CCMD_SET_UNIFORM_BUFFER,

    VIRGL_CCMD_SET_SUB_CTX,
    VIRGL_CCMD_CREATE_SUB_CTX,
    VIRGL_CCMD_DESTROY_SUB_CTX,
    VIRGL_CCMD_BIND_SHADER,
    VIRGL_CCMD_SET_TESS_STATE,
    VIRGL_CCMD_SET_MIN_SAMPLES,
    VIRGL_CCMD_SET_SHADER_BUFFERS,
    VIRGL_CCMD_SET_SHADER_IMAGES,
    VIRGL_CCMD_MEMORY_BARRIER,
    VIRGL_CCMD_LAUNCH_GRID,
    VIRGL_CCMD_SET_FRAMEBUFFER_STATE_NO_ATTACH,
    VIRGL_CCMD_TEXTURE_BARRIER,
    VIRGL_CCMD_SET_ATOMIC_BUFFERS,
    VIRGL_CCMD_SET_DEBUG_FLAGS,
    VIRGL_CCMD_GET_QUERY_RESULT_QBO,
    VIRGL_CCMD_TRANSFER3D,
    VIRGL_CCMD_END_TRANSFERS,
    VIRGL_CCMD_COPY_TRANSFER3D,
    VIRGL_CCMD_SET_TWEAKS,
    VIRGL_CCMD_CLEAR_TEXTURE,
    VIRGL_CCMD_PIPE_RESOURCE_CREATE,
    VIRGL_CCMD_PIPE_RESOURCE_SET_TYPE,
    VIRGL_CCMD_GET_MEMORY_INFO,
    VIRGL_CCMD_SEND_STRING_MARKER,
    VIRGL_CCMD_LINK_SHADER,

    /* video codec */
    VIRGL_CCMD_CREATE_VIDEO_CODEC,
    VIRGL_CCMD_DESTROY_VIDEO_CODEC,
    VIRGL_CCMD_CREATE_VIDEO_BUFFER,
    VIRGL_CCMD_DESTROY_VIDEO_BUFFER,
    VIRGL_CCMD_BEGIN_FRAME,
    VIRGL_CCMD_DECODE_MACROBLOCK,
    VIRGL_CCMD_DECODE_BITSTREAM,
    VIRGL_CCMD_ENCODE_BITSTREAM,
    VIRGL_CCMD_END_FRAME,

    VIRGL_MAX_COMMANDS,
}

const PIPE_CLEAR_DEPTH: u32 = 1 << 0;
const PIPE_CLEAR_STENCIL: u32 = 1 << 1;
const PIPE_CLEAR_COLOR0: u32 = 1 << 2;
const PIPE_CLEAR_COLOR1: u32 = 1 << 3;
const PIPE_CLEAR_COLOR2: u32 = 1 << 4;
const PIPE_CLEAR_COLOR3: u32 = 1 << 5;
const PIPE_CLEAR_COLOR4: u32 = 1 << 6;
const PIPE_CLEAR_COLOR5: u32 = 1 << 7;
const PIPE_CLEAR_COLOR6: u32 = 1 << 8;
const PIPE_CLEAR_COLOR7: u32 = 1 << 9;

/** Combined flags */
/** All color buffers currently bound */
const PIPE_CLEAR_COLOR: u32 = PIPE_CLEAR_COLOR0
    | PIPE_CLEAR_COLOR1
    | PIPE_CLEAR_COLOR2
    | PIPE_CLEAR_COLOR3
    | PIPE_CLEAR_COLOR4
    | PIPE_CLEAR_COLOR5
    | PIPE_CLEAR_COLOR6
    | PIPE_CLEAR_COLOR7;

const PIPE_CLEAR_DEPTHSTENCIL: u32 = PIPE_CLEAR_DEPTH | PIPE_CLEAR_STENCIL;

#[repr(C)]
#[derive(Clone, Debug, Copy)]
pub struct VirglRendererResourceCreateArgs {
    handle: u32,
    target: PipeTextureTarget,
    format: VirglFormats,
    bind: u32,
    width: u32,
    height: u32,
    depth: u32,
    array_size: u32,
    last_level: u32,
    nr_samples: u32,
    flags: u32,
}
impl Default for VirglRendererResourceCreateArgs {
    fn default() -> Self {
        Self {
            handle: 0,
            target: PipeTextureTarget::PIPE_TEXTURE_2D,
            format: VirglFormats::VIRGL_FORMAT_R8G8B8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET,
            width: 128,
            height: 128,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdCtxAttachResource {
    header: VirtioGpuCtrlHdr,
    handle: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
struct VirtioGpuCmdResourceCreate3d {
    header: VirtioGpuCtrlHdr,
    args: VirglRendererResourceCreateArgs,
    padding: u32,
}

const PIPE_BIND_DEPTH_STENCIL: u32 = 1 << 0; // create_surface
const PIPE_BIND_RENDER_TARGET: u32 = 1 << 1; // create_surface
const PIPE_BIND_BLENDABLE: u32 = 1 << 2; // create_surface
const PIPE_BIND_SAMPLER_VIEW: u32 = 1 << 3; // create_sampler_view
const PIPE_BIND_VERTEX_BUFFER: u32 = 1 << 4; // set_vertex_buffers
const PIPE_BIND_INDEX_BUFFER: u32 = 1 << 5; // draw_elements
const PIPE_BIND_CONSTANT_BUFFER: u32 = 1 << 6; // set_constant_buffer
const PIPE_BIND_DISPLAY_TARGET: u32 = 1 << 8; // flush_front_buffer
const PIPE_BIND_TRANSFER_WRITE: u32 = 1 << 9; // transfer_map
const PIPE_BIND_TRANSFER_READ: u32 = 1 << 10; // transfer_map
const PIPE_BIND_STREAM_OUTPUT: u32 = 1 << 11; // set_stream_output_buffers
const PIPE_BIND_CURSOR: u32 = 1 << 16; // mouse cursor
const PIPE_BIND_CUSTOM: u32 = 1 << 17; // state-tracker/winsys usages
const PIPE_BIND_GLOBAL: u32 = 1 << 18; // set_global_binding
const PIPE_BIND_SHADER_RESOURCE: u32 = 1 << 19; // set_shader_resources
const PIPE_BIND_COMPUTE_RESOURCE: u32 = 1 << 20; // set_compute_resources
const PIPE_BIND_COMMAND_ARGS_BUFFER: u32 = 1 << 21; // pipe_draw_info.indirect
const PIPE_BIND_QUERY_BUFFER: u32 = 1 << 22; // get_query_result_resource
const PIPE_BIND_SCANOUT: u32 = 1 << 14;
const PIPE_BIND_SHARED: u32 = 1 << 15;
const PIPE_BIND_LINEAR: u32 = 1 << 21;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum PipeTextureTarget {
    PIPE_BUFFER = 0,
    PIPE_TEXTURE_1D,
    PIPE_TEXTURE_2D,
    PIPE_TEXTURE_3D,
    PIPE_TEXTURE_CUBE,
    PIPE_TEXTURE_RECT,
    PIPE_TEXTURE_1D_ARRAY,
    PIPE_TEXTURE_2D_ARRAY,
    PIPE_TEXTURE_CUBE_ARRAY,
    PIPE_MAX_TEXTURE_TYPES,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum VirglFormats {
    VIRGL_FORMAT_NONE = 0,
    VIRGL_FORMAT_B8G8R8A8_UNORM = 1,
    VIRGL_FORMAT_B8G8R8X8_UNORM = 2,
    VIRGL_FORMAT_A8R8G8B8_UNORM = 3,
    VIRGL_FORMAT_X8R8G8B8_UNORM = 4,
    VIRGL_FORMAT_B5G5R5A1_UNORM = 5,
    VIRGL_FORMAT_B4G4R4A4_UNORM = 6,
    VIRGL_FORMAT_B5G6R5_UNORM = 7,
    VIRGL_FORMAT_R10G10B10A2_UNORM = 8,
    VIRGL_FORMAT_L8_UNORM = 9,
    /**< ubyte luminance */
    VIRGL_FORMAT_A8_UNORM = 10,
    /**< ubyte alpha */
    VIRGL_FORMAT_I8_UNORM = 11,
    VIRGL_FORMAT_L8A8_UNORM = 12,
    /**< ubyte alpha, luminance */
    VIRGL_FORMAT_L16_UNORM = 13,
    /**< ushort luminance */
    VIRGL_FORMAT_UYVY = 14,
    VIRGL_FORMAT_YUYV = 15,
    VIRGL_FORMAT_Z16_UNORM = 16,
    VIRGL_FORMAT_Z32_UNORM = 17,
    VIRGL_FORMAT_Z32_FLOAT = 18,
    VIRGL_FORMAT_Z24_UNORM_S8_UINT = 19,
    VIRGL_FORMAT_S8_UINT_Z24_UNORM = 20,
    VIRGL_FORMAT_Z24X8_UNORM = 21,
    VIRGL_FORMAT_X8Z24_UNORM = 22,
    VIRGL_FORMAT_S8_UINT = 23,
    /**< ubyte stencil */
    VIRGL_FORMAT_R64_FLOAT = 24,
    VIRGL_FORMAT_R64G64_FLOAT = 25,
    VIRGL_FORMAT_R64G64B64_FLOAT = 26,
    VIRGL_FORMAT_R64G64B64A64_FLOAT = 27,
    VIRGL_FORMAT_R32_FLOAT = 28,
    VIRGL_FORMAT_R32G32_FLOAT = 29,
    VIRGL_FORMAT_R32G32B32_FLOAT = 30,
    VIRGL_FORMAT_R32G32B32A32_FLOAT = 31,

    VIRGL_FORMAT_R32_UNORM = 32,
    VIRGL_FORMAT_R32G32_UNORM = 33,
    VIRGL_FORMAT_R32G32B32_UNORM = 34,
    VIRGL_FORMAT_R32G32B32A32_UNORM = 35,
    VIRGL_FORMAT_R32_USCALED = 36,
    VIRGL_FORMAT_R32G32_USCALED = 37,
    VIRGL_FORMAT_R32G32B32_USCALED = 38,
    VIRGL_FORMAT_R32G32B32A32_USCALED = 39,
    VIRGL_FORMAT_R32_SNORM = 40,
    VIRGL_FORMAT_R32G32_SNORM = 41,
    VIRGL_FORMAT_R32G32B32_SNORM = 42,
    VIRGL_FORMAT_R32G32B32A32_SNORM = 43,
    VIRGL_FORMAT_R32_SSCALED = 44,
    VIRGL_FORMAT_R32G32_SSCALED = 45,
    VIRGL_FORMAT_R32G32B32_SSCALED = 46,
    VIRGL_FORMAT_R32G32B32A32_SSCALED = 47,

    VIRGL_FORMAT_R16_UNORM = 48,
    VIRGL_FORMAT_R16G16_UNORM = 49,
    VIRGL_FORMAT_R16G16B16_UNORM = 50,
    VIRGL_FORMAT_R16G16B16A16_UNORM = 51,

    VIRGL_FORMAT_R16_USCALED = 52,
    VIRGL_FORMAT_R16G16_USCALED = 53,
    VIRGL_FORMAT_R16G16B16_USCALED = 54,
    VIRGL_FORMAT_R16G16B16A16_USCALED = 55,

    VIRGL_FORMAT_R16_SNORM = 56,
    VIRGL_FORMAT_R16G16_SNORM = 57,
    VIRGL_FORMAT_R16G16B16_SNORM = 58,
    VIRGL_FORMAT_R16G16B16A16_SNORM = 59,

    VIRGL_FORMAT_R16_SSCALED = 60,
    VIRGL_FORMAT_R16G16_SSCALED = 61,
    VIRGL_FORMAT_R16G16B16_SSCALED = 62,
    VIRGL_FORMAT_R16G16B16A16_SSCALED = 63,

    VIRGL_FORMAT_R8_UNORM = 64,
    VIRGL_FORMAT_R8G8_UNORM = 65,
    VIRGL_FORMAT_R8G8B8_UNORM = 66,
    VIRGL_FORMAT_R8G8B8A8_UNORM = 67,
    VIRGL_FORMAT_X8B8G8R8_UNORM = 68,

    VIRGL_FORMAT_R8_USCALED = 69,
    VIRGL_FORMAT_R8G8_USCALED = 70,
    VIRGL_FORMAT_R8G8B8_USCALED = 71,
    VIRGL_FORMAT_R8G8B8A8_USCALED = 72,

    VIRGL_FORMAT_R8_SNORM = 74,
    VIRGL_FORMAT_R8G8_SNORM = 75,
    VIRGL_FORMAT_R8G8B8_SNORM = 76,
    VIRGL_FORMAT_R8G8B8A8_SNORM = 77,

    VIRGL_FORMAT_R8_SSCALED = 82,
    VIRGL_FORMAT_R8G8_SSCALED = 83,
    VIRGL_FORMAT_R8G8B8_SSCALED = 84,
    VIRGL_FORMAT_R8G8B8A8_SSCALED = 85,

    VIRGL_FORMAT_R32_FIXED = 87,
    VIRGL_FORMAT_R32G32_FIXED = 88,
    VIRGL_FORMAT_R32G32B32_FIXED = 89,
    VIRGL_FORMAT_R32G32B32A32_FIXED = 90,

    VIRGL_FORMAT_R16_FLOAT = 91,
    VIRGL_FORMAT_R16G16_FLOAT = 92,
    VIRGL_FORMAT_R16G16B16_FLOAT = 93,
    VIRGL_FORMAT_R16G16B16A16_FLOAT = 94,

    VIRGL_FORMAT_L8_SRGB = 95,
    VIRGL_FORMAT_L8A8_SRGB = 96,
    VIRGL_FORMAT_R8G8B8_SRGB = 97,
    VIRGL_FORMAT_A8B8G8R8_SRGB = 98,
    VIRGL_FORMAT_X8B8G8R8_SRGB = 99,
    VIRGL_FORMAT_B8G8R8A8_SRGB = 100,
    VIRGL_FORMAT_B8G8R8X8_SRGB = 101,
    VIRGL_FORMAT_A8R8G8B8_SRGB = 102,
    VIRGL_FORMAT_X8R8G8B8_SRGB = 103,
    VIRGL_FORMAT_R8G8B8A8_SRGB = 104,

    /* compressed formats */
    VIRGL_FORMAT_DXT1_RGB = 105,
    VIRGL_FORMAT_DXT1_RGBA = 106,
    VIRGL_FORMAT_DXT3_RGBA = 107,
    VIRGL_FORMAT_DXT5_RGBA = 108,

    /* sRGB, compressed */
    VIRGL_FORMAT_DXT1_SRGB = 109,
    VIRGL_FORMAT_DXT1_SRGBA = 110,
    VIRGL_FORMAT_DXT3_SRGBA = 111,
    VIRGL_FORMAT_DXT5_SRGBA = 112,

    /* rgtc compressed */
    VIRGL_FORMAT_RGTC1_UNORM = 113,
    VIRGL_FORMAT_RGTC1_SNORM = 114,
    VIRGL_FORMAT_RGTC2_UNORM = 115,
    VIRGL_FORMAT_RGTC2_SNORM = 116,

    VIRGL_FORMAT_R8G8_B8G8_UNORM = 117,
    VIRGL_FORMAT_G8R8_G8B8_UNORM = 118,

    VIRGL_FORMAT_R8SG8SB8UX8U_NORM = 119,
    VIRGL_FORMAT_R5SG5SB6U_NORM = 120,

    VIRGL_FORMAT_A8B8G8R8_UNORM = 121,
    VIRGL_FORMAT_B5G5R5X1_UNORM = 122,
    VIRGL_FORMAT_R10G10B10A2_USCALED = 123,
    VIRGL_FORMAT_R11G11B10_FLOAT = 124,
    VIRGL_FORMAT_R9G9B9E5_FLOAT = 125,
    VIRGL_FORMAT_Z32_FLOAT_S8X24_UINT = 126,
    VIRGL_FORMAT_R1_UNORM = 127,
    VIRGL_FORMAT_R10G10B10X2_USCALED = 128,
    VIRGL_FORMAT_R10G10B10X2_SNORM = 129,

    VIRGL_FORMAT_L4A4_UNORM = 130,
    VIRGL_FORMAT_B10G10R10A2_UNORM = 131,
    VIRGL_FORMAT_R10SG10SB10SA2U_NORM = 132,
    VIRGL_FORMAT_R8G8Bx_SNORM = 133,
    VIRGL_FORMAT_R8G8B8X8_UNORM = 134,
    VIRGL_FORMAT_B4G4R4X4_UNORM = 135,
    VIRGL_FORMAT_X24S8_UINT = 136,
    VIRGL_FORMAT_S8X24_UINT = 137,
    VIRGL_FORMAT_X32_S8X24_UINT = 138,
    VIRGL_FORMAT_B2G3R3_UNORM = 139,

    VIRGL_FORMAT_L16A16_UNORM = 140,
    VIRGL_FORMAT_A16_UNORM = 141,
    VIRGL_FORMAT_I16_UNORM = 142,

    VIRGL_FORMAT_LATC1_UNORM = 143,
    VIRGL_FORMAT_LATC1_SNORM = 144,
    VIRGL_FORMAT_LATC2_UNORM = 145,
    VIRGL_FORMAT_LATC2_SNORM = 146,

    VIRGL_FORMAT_A8_SNORM = 147,
    VIRGL_FORMAT_L8_SNORM = 148,
    VIRGL_FORMAT_L8A8_SNORM = 149,
    VIRGL_FORMAT_I8_SNORM = 150,
    VIRGL_FORMAT_A16_SNORM = 151,
    VIRGL_FORMAT_L16_SNORM = 152,
    VIRGL_FORMAT_L16A16_SNORM = 153,
    VIRGL_FORMAT_I16_SNORM = 154,

    VIRGL_FORMAT_A16_FLOAT = 155,
    VIRGL_FORMAT_L16_FLOAT = 156,
    VIRGL_FORMAT_L16A16_FLOAT = 157,
    VIRGL_FORMAT_I16_FLOAT = 158,
    VIRGL_FORMAT_A32_FLOAT = 159,
    VIRGL_FORMAT_L32_FLOAT = 160,
    VIRGL_FORMAT_L32A32_FLOAT = 161,
    VIRGL_FORMAT_I32_FLOAT = 162,

    VIRGL_FORMAT_YV12 = 163,
    VIRGL_FORMAT_YV16 = 164,
    VIRGL_FORMAT_IYUV = 165,
    /**< aka I420 */
    VIRGL_FORMAT_NV12 = 166,
    VIRGL_FORMAT_NV21 = 167,

    VIRGL_FORMAT_A4R4_UNORM = 168,
    VIRGL_FORMAT_R4A4_UNORM = 169,
    VIRGL_FORMAT_R8A8_UNORM = 170,
    VIRGL_FORMAT_A8R8_UNORM = 171,

    VIRGL_FORMAT_R10G10B10A2_SSCALED = 172,
    VIRGL_FORMAT_R10G10B10A2_SNORM = 173,
    VIRGL_FORMAT_B10G10R10A2_USCALED = 174,
    VIRGL_FORMAT_B10G10R10A2_SSCALED = 175,
    VIRGL_FORMAT_B10G10R10A2_SNORM = 176,

    VIRGL_FORMAT_R8_UINT = 177,
    VIRGL_FORMAT_R8G8_UINT = 178,
    VIRGL_FORMAT_R8G8B8_UINT = 179,
    VIRGL_FORMAT_R8G8B8A8_UINT = 180,

    VIRGL_FORMAT_R8_SINT = 181,
    VIRGL_FORMAT_R8G8_SINT = 182,
    VIRGL_FORMAT_R8G8B8_SINT = 183,
    VIRGL_FORMAT_R8G8B8A8_SINT = 184,

    VIRGL_FORMAT_R16_UINT = 185,
    VIRGL_FORMAT_R16G16_UINT = 186,
    VIRGL_FORMAT_R16G16B16_UINT = 187,
    VIRGL_FORMAT_R16G16B16A16_UINT = 188,

    VIRGL_FORMAT_R16_SINT = 189,
    VIRGL_FORMAT_R16G16_SINT = 190,
    VIRGL_FORMAT_R16G16B16_SINT = 191,
    VIRGL_FORMAT_R16G16B16A16_SINT = 192,
    VIRGL_FORMAT_R32_UINT = 193,
    VIRGL_FORMAT_R32G32_UINT = 194,
    VIRGL_FORMAT_R32G32B32_UINT = 195,
    VIRGL_FORMAT_R32G32B32A32_UINT = 196,

    VIRGL_FORMAT_R32_SINT = 197,
    VIRGL_FORMAT_R32G32_SINT = 198,
    VIRGL_FORMAT_R32G32B32_SINT = 199,
    VIRGL_FORMAT_R32G32B32A32_SINT = 200,

    VIRGL_FORMAT_A8_UINT = 201,
    VIRGL_FORMAT_I8_UINT = 202,
    VIRGL_FORMAT_L8_UINT = 203,
    VIRGL_FORMAT_L8A8_UINT = 204,

    VIRGL_FORMAT_A8_SINT = 205,
    VIRGL_FORMAT_I8_SINT = 206,
    VIRGL_FORMAT_L8_SINT = 207,
    VIRGL_FORMAT_L8A8_SINT = 208,

    VIRGL_FORMAT_A16_UINT = 209,
    VIRGL_FORMAT_I16_UINT = 210,
    VIRGL_FORMAT_L16_UINT = 211,
    VIRGL_FORMAT_L16A16_UINT = 212,

    VIRGL_FORMAT_A16_SINT = 213,
    VIRGL_FORMAT_I16_SINT = 214,
    VIRGL_FORMAT_L16_SINT = 215,
    VIRGL_FORMAT_L16A16_SINT = 216,

    VIRGL_FORMAT_A32_UINT = 217,
    VIRGL_FORMAT_I32_UINT = 218,
    VIRGL_FORMAT_L32_UINT = 219,
    VIRGL_FORMAT_L32A32_UINT = 220,

    VIRGL_FORMAT_A32_SINT = 221,
    VIRGL_FORMAT_I32_SINT = 222,
    VIRGL_FORMAT_L32_SINT = 223,
    VIRGL_FORMAT_L32A32_SINT = 224,

    VIRGL_FORMAT_B10G10R10A2_UINT = 225,
    VIRGL_FORMAT_ETC1_RGB8 = 226,
    VIRGL_FORMAT_R8G8_R8B8_UNORM = 227,
    VIRGL_FORMAT_G8R8_B8R8_UNORM = 228,
    VIRGL_FORMAT_R8G8B8X8_SNORM = 229,

    VIRGL_FORMAT_R8G8B8X8_SRGB = 230,

    VIRGL_FORMAT_R8G8B8X8_UINT = 231,
    VIRGL_FORMAT_R8G8B8X8_SINT = 232,
    VIRGL_FORMAT_B10G10R10X2_UNORM = 233,
    VIRGL_FORMAT_R16G16B16X16_UNORM = 234,
    VIRGL_FORMAT_R16G16B16X16_SNORM = 235,
    VIRGL_FORMAT_R16G16B16X16_FLOAT = 236,
    VIRGL_FORMAT_R16G16B16X16_UINT = 237,
    VIRGL_FORMAT_R16G16B16X16_SINT = 238,
    VIRGL_FORMAT_R32G32B32X32_FLOAT = 239,
    VIRGL_FORMAT_R32G32B32X32_UINT = 240,
    VIRGL_FORMAT_R32G32B32X32_SINT = 241,
    VIRGL_FORMAT_R8A8_SNORM = 242,
    VIRGL_FORMAT_R16A16_UNORM = 243,
    VIRGL_FORMAT_R16A16_SNORM = 244,
    VIRGL_FORMAT_R16A16_FLOAT = 245,
    VIRGL_FORMAT_R32A32_FLOAT = 246,
    VIRGL_FORMAT_R8A8_UINT = 247,
    VIRGL_FORMAT_R8A8_SINT = 248,
    VIRGL_FORMAT_R16A16_UINT = 249,
    VIRGL_FORMAT_R16A16_SINT = 250,
    VIRGL_FORMAT_R32A32_UINT = 251,
    VIRGL_FORMAT_R32A32_SINT = 252,

    VIRGL_FORMAT_R10G10B10A2_UINT = 253,
    VIRGL_FORMAT_B5G6R5_SRGB = 254,

    VIRGL_FORMAT_BPTC_RGBA_UNORM = 255,
    VIRGL_FORMAT_BPTC_SRGBA = 256,
    VIRGL_FORMAT_BPTC_RGB_FLOAT = 257,
    VIRGL_FORMAT_BPTC_RGB_UFLOAT = 258,

    VIRGL_FORMAT_A16L16_UNORM = 262,

    VIRGL_FORMAT_G8R8_UNORM = 263,
    VIRGL_FORMAT_G8R8_SNORM = 264,
    VIRGL_FORMAT_G16R16_UNORM = 265,
    VIRGL_FORMAT_G16R16_SNORM = 266,
    VIRGL_FORMAT_A8B8G8R8_SNORM = 267,

    VIRGL_FORMAT_A8L8_UNORM = 259,
    VIRGL_FORMAT_A8L8_SNORM = 260,
    VIRGL_FORMAT_A8L8_SRGB = 261,

    // VIRGL_FORMAT_A1B5G5R5_UNORM = 262,
    // VIRGL_FORMAT_A1R5G5B5_UNORM = 263,
    // VIRGL_FORMAT_A2B10G10R10_UNORM = 264,
    // VIRGL_FORMAT_A2R10G10B10_UNORM = 265,
    // VIRGL_FORMAT_A4R4G4B4_UNORM = 266,
    VIRGL_FORMAT_X8B8G8R8_SNORM = 268,

    /* etc2 compressed */
    VIRGL_FORMAT_ETC2_RGB8 = 269,
    VIRGL_FORMAT_ETC2_SRGB8 = 270,
    VIRGL_FORMAT_ETC2_RGB8A1 = 271,
    VIRGL_FORMAT_ETC2_SRGB8A1 = 272,
    VIRGL_FORMAT_ETC2_RGBA8 = 273,
    VIRGL_FORMAT_ETC2_SRGBA8 = 274,
    VIRGL_FORMAT_ETC2_R11_UNORM = 275,
    VIRGL_FORMAT_ETC2_R11_SNORM = 276,
    VIRGL_FORMAT_ETC2_RG11_UNORM = 277,
    VIRGL_FORMAT_ETC2_RG11_SNORM = 278,

    VIRGL_FORMAT_ASTC_4x4 = 279,
    VIRGL_FORMAT_ASTC_5x4 = 280,
    VIRGL_FORMAT_ASTC_5x5 = 281,
    VIRGL_FORMAT_ASTC_6x5 = 282,
    VIRGL_FORMAT_ASTC_6x6 = 283,
    VIRGL_FORMAT_ASTC_8x5 = 284,
    VIRGL_FORMAT_ASTC_8x6 = 285,
    VIRGL_FORMAT_ASTC_8x8 = 286,
    VIRGL_FORMAT_ASTC_10x5 = 287,
    VIRGL_FORMAT_ASTC_10x6 = 288,
    VIRGL_FORMAT_ASTC_10x8 = 289,
    VIRGL_FORMAT_ASTC_10x10 = 290,
    VIRGL_FORMAT_ASTC_12x10 = 291,
    VIRGL_FORMAT_ASTC_12x12 = 292,
    VIRGL_FORMAT_ASTC_4x4_SRGB = 293,
    VIRGL_FORMAT_ASTC_5x4_SRGB = 294,
    VIRGL_FORMAT_ASTC_5x5_SRGB = 295,
    VIRGL_FORMAT_ASTC_6x5_SRGB = 296,
    VIRGL_FORMAT_ASTC_6x6_SRGB = 297,
    VIRGL_FORMAT_ASTC_8x5_SRGB = 298,
    VIRGL_FORMAT_ASTC_8x6_SRGB = 299,
    VIRGL_FORMAT_ASTC_8x8_SRGB = 300,
    VIRGL_FORMAT_ASTC_10x5_SRGB = 301,
    VIRGL_FORMAT_ASTC_10x6_SRGB = 302,
    VIRGL_FORMAT_ASTC_10x8_SRGB = 303,
    VIRGL_FORMAT_ASTC_10x10_SRGB = 304,
    VIRGL_FORMAT_ASTC_12x10_SRGB = 305,
    VIRGL_FORMAT_ASTC_12x12_SRGB = 306,

    VIRGL_FORMAT_R10G10B10X2_UNORM = 308,
    VIRGL_FORMAT_A4B4G4R4_UNORM = 311,

    VIRGL_FORMAT_R8_SRGB = 312,
    VIRGL_FORMAT_R8G8_SRGB = 313,

    VIRGL_FORMAT_P010 = 314,
    VIRGL_FORMAT_P012 = 315,
    VIRGL_FORMAT_P016 = 316,

    VIRGL_FORMAT_B8G8R8_UNORM = 317,
    VIRGL_FORMAT_R3G3B2_UNORM = 318,
    VIRGL_FORMAT_R4G4B4A4_UNORM = 319,
    VIRGL_FORMAT_R5G5B5A1_UNORM = 320,
    VIRGL_FORMAT_R5G6B5_UNORM = 321,

    VIRGL_FORMAT_MAX, /* = PIPE_FORMAT_COUNT */

    /* Below formats must not be used in the guest. */
    VIRGL_FORMAT_B8G8R8X8_UNORM_EMULATED,
    VIRGL_FORMAT_B8G8R8A8_UNORM_EMULATED,
    VIRGL_FORMAT_MAX_EXTENDED,
}

impl VirglFormats {
    pub const VIRGL_FORMAT_A1B5G5R5_UNORM: VirglFormats = VirglFormats::VIRGL_FORMAT_A16L16_UNORM;
    pub const VIRGL_FORMAT_A1R5G5B5_UNORM: VirglFormats = VirglFormats::VIRGL_FORMAT_G8R8_UNORM;
    pub const VIRGL_FORMAT_A2B10G10R10_UNORM: VirglFormats = VirglFormats::VIRGL_FORMAT_G8R8_SNORM;
    pub const VIRGL_FORMAT_A2R10G10B10_UNORM: VirglFormats =
        VirglFormats::VIRGL_FORMAT_G16R16_UNORM;
    pub const VIRGL_FORMAT_A4R4G4B4_UNORM: VirglFormats = VirglFormats::VIRGL_FORMAT_G16R16_SNORM;
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum VirglObjectType {
    VIRGL_OBJECT_NULL,
    VIRGL_OBJECT_BLEND,
    VIRGL_OBJECT_RASTERIZER,
    VIRGL_OBJECT_DSA,
    VIRGL_OBJECT_SHADER,
    VIRGL_OBJECT_VERTEX_ELEMENTS,
    VIRGL_OBJECT_SAMPLER_VIEW,
    VIRGL_OBJECT_SAMPLER_STATE,
    VIRGL_OBJECT_SURFACE,
    VIRGL_OBJECT_QUERY,
    VIRGL_OBJECT_STREAMOUT_TARGET,
    VIRGL_OBJECT_MSAA_SURFACE,
    VIRGL_MAX_OBJECTS,
}
