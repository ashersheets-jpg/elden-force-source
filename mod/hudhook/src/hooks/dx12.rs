//! Hooks for DirectX 12.

use std::ffi::c_void;
use std::fmt::{self, Display};
use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use imgui::Context;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use tracing::{debug, error, trace, warn};
use windows::core::{Error, IUnknown, Interface, Result, BOOL, HRESULT};
use windows::Win32::Foundation::{HWND, LUID};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12CreateDevice, ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12Resource,
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_COMMAND_QUEUE_FLAG_NONE,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED,
    DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory, IDXGIFactory2, IDXGIOutput, IDXGISwapChain, IDXGISwapChain1,
    IDXGISwapChain2, IDXGISwapChain3, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_DEVICE_REMOVED,
    DXGI_ERROR_DEVICE_RESET, DXGI_PRESENT_PARAMETERS, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH, DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use super::DummyHwnd;
use crate::mh::MhHook;
use crate::renderer::{D3D12RenderEngine, Pipeline};
use crate::{perform_eject, util, Hooks, ImguiRenderLoop, EJECT_REQUESTED, HOOK_EJECTION_BARRIER};

type DXGISwapChainPresentType =
    unsafe extern "system" fn(this: IDXGISwapChain, sync_interval: u32, flags: u32) -> HRESULT;

type DXGISwapChainPresent1Type = unsafe extern "system" fn(
    this: IDXGISwapChain1,
    sync_interval: u32,
    present_flags: u32,
    present_parameters: *const DXGI_PRESENT_PARAMETERS,
) -> HRESULT;

type DXGISwapChainResizeBuffersType = unsafe extern "system" fn(
    this: IDXGISwapChain,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: DXGI_FORMAT,
    flags: u32,
) -> HRESULT;

type DXGISwapChainSetSourceSizeType =
    unsafe extern "system" fn(this: IDXGISwapChain2, width: u32, height: u32) -> HRESULT;

type DXGISwapChainResizeBuffers1Type = unsafe extern "system" fn(
    this: IDXGISwapChain3,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: DXGI_FORMAT,
    flags: u32,
    creation_node_mask: *const u32,
    present_queue: *const Option<IUnknown>,
) -> HRESULT;

type DXGIFactoryCreateSwapChainType = unsafe extern "system" fn(
    this: IDXGIFactory,
    device: IUnknown,
    desc: *const DXGI_SWAP_CHAIN_DESC,
    swap_chain: *mut Option<IDXGISwapChain>,
) -> HRESULT;

type DXGIFactoryCreateSwapChainForHwndType = unsafe extern "system" fn(
    this: IDXGIFactory2,
    device: IUnknown,
    hwnd: HWND,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen_desc: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    restrict_to_output: Option<IDXGIOutput>,
    swap_chain: *mut Option<IDXGISwapChain1>,
) -> HRESULT;

type D3D12CommandQueueExecuteCommandListsType = unsafe extern "system" fn(
    this: ID3D12CommandQueue,
    num_command_lists: u32,
    command_lists: *mut ID3D12CommandList,
);

struct Trampolines {
    dxgi_swap_chain_present: DXGISwapChainPresentType,
    dxgi_swap_chain_present1: DXGISwapChainPresent1Type,
    dxgi_swap_chain_resize_buffers: DXGISwapChainResizeBuffersType,
    dxgi_swap_chain_set_source_size: DXGISwapChainSetSourceSizeType,
    dxgi_swap_chain_resize_buffers1: DXGISwapChainResizeBuffers1Type,
    dxgi_factory_create_swap_chain: DXGIFactoryCreateSwapChainType,
    dxgi_factory_create_swap_chain_for_hwnd: DXGIFactoryCreateSwapChainForHwndType,
    d3d12_command_queue_execute_command_lists: D3D12CommandQueueExecuteCommandListsType,
}

static mut TRAMPOLINES: OnceLock<Trampolines> = OnceLock::new();

// (Elden Force 2.0.7: at start only the factory's two swap-chain makers are hooked, found from a DXGI factory alone, with
// no device, queue, swap chain or window of our own made while the game sets up its graphics. The rest are hooked
// from the game's own swap chain and queue as it makes them (late_hook); TRAMPOLINES is set then.)
#[derive(Clone, Copy)]
struct FactoryTrampolines {
    create_swap_chain: DXGIFactoryCreateSwapChainType,
    create_swap_chain_for_hwnd: DXGIFactoryCreateSwapChainForHwndType,
}
static FACTORY: OnceLock<FactoryTrampolines> = OnceLock::new();
static LATE_LOCK: Mutex<()> = Mutex::new(());
static LATE_DONE: AtomicBool = AtomicBool::new(false);

enum InitializationContext {
    Empty,
    WithSwapChain(IDXGISwapChain3),
    Complete(IDXGISwapChain3, ID3D12CommandQueue),
    Done,
}

impl InitializationContext {
    // Transition to a state where the swap chain is set. Ignore other
    // mutations.
    fn insert_swap_chain(&mut self, swap_chain: &IDXGISwapChain3) {
        *self = match mem::replace(self, InitializationContext::Empty) {
            InitializationContext::Empty => {
                InitializationContext::WithSwapChain(swap_chain.clone())
            },
            s => s,
        }
    }

    // Transition to a complete state with a command queue that was provided by
    // a DXGI factory/present-queue API and therefore does not need private
    // swap-chain layout probing.
    fn set_complete(&mut self, swap_chain: &IDXGISwapChain3, command_queue: &ID3D12CommandQueue) {
        if matches!(self, InitializationContext::Done) {
            return;
        }

        trace!(
            "Found command queue from swap chain creation path {swap_chain:?} at {command_queue:?}"
        );
        *self = InitializationContext::Complete(swap_chain.clone(), command_queue.clone());
    }

    // Transition to a complete state if the swap chain is set and the command
    // queue is associated with it.
    fn insert_command_queue(&mut self, command_queue: &ID3D12CommandQueue) {
        *self = match mem::replace(self, InitializationContext::Empty) {
            InitializationContext::WithSwapChain(swap_chain) => {
                if unsafe { Self::check_command_queue(&swap_chain, command_queue) } {
                    trace!(
                        "Found command queue matching swap chain {swap_chain:?} at \
                         {command_queue:?}"
                    );
                    InitializationContext::Complete(swap_chain, command_queue.clone())
                } else {
                    InitializationContext::WithSwapChain(swap_chain)
                }
            },
            s => s,
        }
    }

    // Retrieve the values if the context is complete.
    fn get(&self) -> Option<(IDXGISwapChain3, ID3D12CommandQueue)> {
        if let InitializationContext::Complete(swap_chain, command_queue) = self {
            Some((swap_chain.clone(), command_queue.clone()))
        } else {
            None
        }
    }

    // Mark the context as done so no further operations are executed on it.
    fn done(&mut self) {
        if let InitializationContext::Complete(..) = self {
            *self = InitializationContext::Done;
        }
    }

    unsafe fn check_command_queue(
        swap_chain: &IDXGISwapChain3,
        command_queue: &ID3D12CommandQueue,
    ) -> bool {
        let swap_chain_ptr = swap_chain.as_raw() as *mut *mut c_void;
        let readable_ptrs = util::readable_region(swap_chain_ptr, 512);

        match readable_ptrs.iter().position(|&ptr| std::ptr::eq(ptr, command_queue.as_raw())) {
            Some(idx) => {
                debug!(
                    "Found command queue pointer in swap chain struct at offset +0x{:x}",
                    idx * mem::size_of::<usize>(),
                );
                true
            },
            None => {
                warn!(
                    "Couldn't find command queue pointer in swap chain struct ({} out of 512 \
                     pointers were readable)",
                    readable_ptrs.len()
                );
                false
            },
        }
    }
}

/// Wraps the initialization state machine with a Mutex and tracks completion
/// via an internal AtomicBool so callers can skip the lock on the hot path.
struct InitState {
    inner: Mutex<InitializationContext>,
    done: AtomicBool,
}

impl InitState {
    const fn new() -> Self {
        Self { inner: Mutex::new(InitializationContext::Empty), done: AtomicBool::new(false) }
    }

    fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    fn lock(&self) -> parking_lot::MutexGuard<'_, InitializationContext> {
        self.inner.lock()
    }

    fn mark_done(&self) {
        self.inner.lock().done();
        self.done.store(true, Ordering::Release);
    }

    fn reset(&self) {
        *self.inner.lock() = InitializationContext::Empty;
        self.done.store(false, Ordering::Release);
    }
}

static INIT_STATE: InitState = InitState::new();
static mut PIPELINE: OnceCell<Mutex<Pipeline<D3D12RenderEngine>>> = OnceCell::new();
static mut RENDER_LOOP: OnceCell<Box<dyn ImguiRenderLoop + Send + Sync>> = OnceCell::new();
static ACTIVE_CONTEXT: Mutex<Option<ActiveDx12Context>> = Mutex::new(None);

#[derive(Clone)]
struct ActiveDx12Context {
    swap_chain_identity: usize,
    device_identity: usize,
    command_queue: ID3D12CommandQueue,
    command_queue_device_identity: usize,
    adapter_luid: LUID,
    rtv_format: DXGI_FORMAT,
    buffer_count: u32,
    hwnd: usize,
}

struct FmtLuid(LUID);

impl Display for FmtLuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:08x}:{:08x}", self.0.HighPart, self.0.LowPart)
    }
}

fn identity_ptr(identity: &IUnknown) -> usize {
    identity.as_raw() as usize
}

fn command_queue_from_unknown(device: &IUnknown) -> Option<ID3D12CommandQueue> {
    let Ok(command_queue) = device.cast::<ID3D12CommandQueue>() else {
        return None;
    };

    let desc = unsafe { command_queue.GetDesc() };
    if desc.Type == D3D12_COMMAND_LIST_TYPE_DIRECT {
        Some(command_queue)
    } else {
        None
    }
}

enum PresentQueueCapture {
    Missing,
    Valid(ID3D12CommandQueue),
    Invalid,
}

fn command_queue_from_present_queues(
    present_queue: *const Option<IUnknown>,
    creation_node_mask: *const u32,
    buffer_count: u32,
) -> PresentQueueCapture {
    if present_queue.is_null() {
        return PresentQueueCapture::Missing;
    }

    if buffer_count == 0 {
        warn!("ResizeBuffers1 supplied present queues but the queue count is unknown");
        return PresentQueueCapture::Invalid;
    }

    let present_queues =
        unsafe { std::slice::from_raw_parts(present_queue, buffer_count as usize) };
    let creation_node_masks = if creation_node_mask.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(creation_node_mask, buffer_count as usize) })
    };

    let Some(first_unknown) = present_queues.first().and_then(Option::as_ref) else {
        warn!("ResizeBuffers1 present queue array contains a null first queue");
        return PresentQueueCapture::Invalid;
    };
    let Some(first_queue) = command_queue_from_unknown(first_unknown) else {
        warn!("ResizeBuffers1 first present queue is not a direct D3D12 command queue");
        return PresentQueueCapture::Invalid;
    };
    let Ok(first_queue_identity) = first_queue.cast::<IUnknown>() else {
        warn!("ResizeBuffers1 first present queue identity query failed");
        return PresentQueueCapture::Invalid;
    };
    let first_queue_identity = identity_ptr(&first_queue_identity);
    let first_node_mask = creation_node_masks.and_then(|masks| masks.first().copied());

    for (idx, present_queue) in present_queues.iter().enumerate().skip(1) {
        let Some(present_queue) = present_queue.as_ref() else {
            warn!("ResizeBuffers1 present queue array contains a null queue at index {idx}");
            return PresentQueueCapture::Invalid;
        };
        let Some(command_queue) = command_queue_from_unknown(present_queue) else {
            warn!(
                "ResizeBuffers1 present queue at index {idx} is not a direct D3D12 command queue"
            );
            return PresentQueueCapture::Invalid;
        };
        let Ok(command_queue_identity) = command_queue.cast::<IUnknown>() else {
            warn!("ResizeBuffers1 present queue identity query failed at index {idx}");
            return PresentQueueCapture::Invalid;
        };
        if identity_ptr(&command_queue_identity) != first_queue_identity {
            warn!(
                "ResizeBuffers1 supplied multiple distinct present queues; skipping DX12 overlay \
                 reinitialization"
            );
            return PresentQueueCapture::Invalid;
        }

        if let (Some(masks), Some(first_node_mask)) = (creation_node_masks, first_node_mask) {
            if masks[idx] != first_node_mask {
                warn!(
                    "ResizeBuffers1 supplied different creation node masks; skipping DX12 overlay \
                     reinitialization"
                );
                return PresentQueueCapture::Invalid;
            }
        }
    }

    PresentQueueCapture::Valid(first_queue)
}

fn create_active_context(
    swap_chain: &IDXGISwapChain3,
    command_queue: &ID3D12CommandQueue,
    swap_chain_desc: &DXGI_SWAP_CHAIN_DESC,
    rtv_format: DXGI_FORMAT,
) -> Result<ActiveDx12Context> {
    let swap_chain_identity: IUnknown = swap_chain.cast()?;
    let device: ID3D12Device = unsafe { swap_chain.GetDevice()? };
    let device_identity: IUnknown = device.cast()?;
    let queue_device: ID3D12Device = util::try_out_ptr(|v| unsafe { command_queue.GetDevice(v) })?;
    let command_queue_device_identity: IUnknown = queue_device.cast()?;
    let adapter_luid = unsafe { device.GetAdapterLuid() };
    let hwnd = swap_chain_desc.OutputWindow.0 as usize;

    Ok(ActiveDx12Context {
        swap_chain_identity: identity_ptr(&swap_chain_identity),
        device_identity: identity_ptr(&device_identity),
        command_queue: command_queue.clone(),
        command_queue_device_identity: identity_ptr(&command_queue_device_identity),
        adapter_luid,
        rtv_format,
        buffer_count: swap_chain_desc.BufferCount,
        hwnd,
    })
}

unsafe fn reset_pipeline(reason: &str) {
    warn!("Resetting DX12 pipeline: {reason}");

    if let Some(pipeline) = PIPELINE.take() {
        let render_loop = pipeline.into_inner().take();
        if RENDER_LOOP.set(render_loop).is_err() {
            error!("Render loop cell was not empty while resetting DX12 pipeline");
        }
    }

    *ACTIVE_CONTEXT.lock() = None;
    INIT_STATE.reset();
}

fn validate_pending_initialization_context(swap_chain: &IDXGISwapChain3) -> Result<bool> {
    if ACTIVE_CONTEXT.lock().is_some() {
        return Ok(true);
    }

    let Some((captured_swap_chain, _)) = ({ INIT_STATE.lock().get() }) else {
        return Ok(true);
    };

    let captured_identity: IUnknown = captured_swap_chain.cast()?;
    let current_identity: IUnknown = swap_chain.cast()?;
    if identity_ptr(&captured_identity) == identity_ptr(&current_identity) {
        return Ok(true);
    }

    warn!("DX12 pending initialization context belongs to a different swap chain; resetting");
    INIT_STATE.reset();
    INIT_STATE.lock().insert_swap_chain(swap_chain);
    Ok(false)
}

fn validate_active_context(swap_chain: &IDXGISwapChain3) -> Result<bool> {
    let Some(active) = ACTIVE_CONTEXT.lock().clone() else {
        return Ok(true);
    };

    let Ok(current_swap_chain_identity) = swap_chain.cast::<IUnknown>() else {
        unsafe { reset_pipeline("swap-chain identity query failed") };
        return Ok(false);
    };
    if identity_ptr(&current_swap_chain_identity) != active.swap_chain_identity {
        unsafe { reset_pipeline("swap-chain replacement") };
        return Ok(false);
    }

    let current_device: ID3D12Device = match unsafe { swap_chain.GetDevice() } {
        Ok(device) => device,
        Err(e) => {
            warn!("Could not query DX12 swap-chain device: {e:?}");
            unsafe { reset_pipeline("swap-chain device query failed") };
            return Ok(false);
        },
    };
    let Ok(current_device_identity) = current_device.cast::<IUnknown>() else {
        unsafe { reset_pipeline("swap-chain device identity query failed") };
        return Ok(false);
    };
    let current_device_identity_ptr = identity_ptr(&current_device_identity);
    if current_device_identity_ptr != active.device_identity {
        warn!(
            "DX12 swap-chain device changed; old adapter LUID {}, new adapter LUID {}",
            FmtLuid(active.adapter_luid),
            FmtLuid(unsafe { current_device.GetAdapterLuid() }),
        );
        unsafe { reset_pipeline("swap-chain device replacement") };
        return Ok(false);
    }

    let queue_device: ID3D12Device =
        match util::try_out_ptr(|v| unsafe { active.command_queue.GetDevice(v) }) {
            Ok(device) => device,
            Err(e) => {
                warn!("Could not query DX12 command queue device: {e:?}");
                unsafe { reset_pipeline("command queue device query failed") };
                return Ok(false);
            },
        };
    let Ok(queue_device_identity) = queue_device.cast::<IUnknown>() else {
        unsafe { reset_pipeline("command queue device identity query failed") };
        return Ok(false);
    };
    let queue_device_identity_ptr = identity_ptr(&queue_device_identity);
    if queue_device_identity_ptr != current_device_identity_ptr
        || queue_device_identity_ptr != active.command_queue_device_identity
    {
        warn!(
            "DX12 command queue device no longer matches swap-chain device; adapter LUID {}",
            FmtLuid(active.adapter_luid),
        );
        unsafe { reset_pipeline("command queue device mismatch") };
        return Ok(false);
    }

    let device_removed_reason = unsafe { current_device.GetDeviceRemovedReason() };
    if device_removed_reason.is_err() {
        warn!("DX12 device removed/reset: {device_removed_reason:?}");
        unsafe { reset_pipeline("device removed") };
        return Ok(false);
    }

    let swap_chain_desc = unsafe { swap_chain.GetDesc()? };
    let Some(current_rtv_format) =
        D3D12RenderEngine::rtv_format_for_swap_chain(swap_chain_desc.BufferDesc.Format)
    else {
        warn!(
            "DX12 swap-chain format {:?} is not supported by hudhook; skipping overlay",
            swap_chain_desc.BufferDesc.Format,
        );
        unsafe { reset_pipeline("unsupported swap-chain format") };
        return Ok(false);
    };
    if current_rtv_format != active.rtv_format || swap_chain_desc.BufferCount != active.buffer_count
    {
        let command_queue = active.command_queue.clone();
        unsafe {
            reset_pipeline("swap-chain format or buffer count changed");
            complete_initialization_from_queue(swap_chain, &command_queue);
        }
        return Ok(false);
    }

    Ok(true)
}

unsafe fn handle_present_result(where_: &str, result: HRESULT) {
    if result == DXGI_ERROR_DEVICE_REMOVED || result == DXGI_ERROR_DEVICE_RESET {
        reset_pipeline(where_);
    }
}

unsafe fn complete_initialization_from_queue(
    swap_chain: &IDXGISwapChain3,
    command_queue: &ID3D12CommandQueue,
) {
    INIT_STATE.lock().set_complete(swap_chain, command_queue);
}

unsafe fn handle_swap_chain_created(
    hwnd: HWND,
    device: &IUnknown,
    swap_chain: Option<IDXGISwapChain3>,
    result: HRESULT,
) {
    if result.is_err() {
        return;
    }

    let Some(swap_chain) = swap_chain else {
        return;
    };

    let Some(command_queue) = command_queue_from_unknown(device) else {
        return;
    };

    late_hook_from(&swap_chain, &command_queue);

    if let Some(active) = ACTIVE_CONTEXT.lock().clone() {
        let hwnd = hwnd.0 as usize;
        if hwnd != 0 && active.hwnd != 0 && hwnd != active.hwnd {
            return;
        }

        if let Ok(current_identity) = swap_chain.cast::<IUnknown>() {
            if identity_ptr(&current_identity) != active.swap_chain_identity {
                reset_pipeline("swap-chain creation replacement");
            }
        }
    }

    trace!("Captured DX12 command queue from swap-chain creation for HWND {hwnd:?}");
    complete_initialization_from_queue(&swap_chain, &command_queue);
}

unsafe fn init_pipeline() -> Result<Mutex<Pipeline<D3D12RenderEngine>>> {
    let Some((swap_chain, command_queue)) = ({ INIT_STATE.lock().get() }) else {
        error!("Initialization context incomplete");
        return Err(Error::from_hresult(HRESULT(-1)));
    };

    let swap_chain_desc = unsafe { swap_chain.GetDesc() }?;
    let Some(rtv_format) =
        D3D12RenderEngine::rtv_format_for_swap_chain(swap_chain_desc.BufferDesc.Format)
    else {
        warn!(
            "DX12 swap-chain format {:?} is not supported by hudhook; skipping overlay",
            swap_chain_desc.BufferDesc.Format,
        );
        return Err(Error::from_hresult(HRESULT(-1)));
    };
    let hwnd = swap_chain_desc.OutputWindow;
    let active_context =
        create_active_context(&swap_chain, &command_queue, &swap_chain_desc, rtv_format)?;

    let mut ctx = Context::create();
    let engine = D3D12RenderEngine::new(&command_queue, &mut ctx, rtv_format)?;

    let Some(render_loop) = RENDER_LOOP.take() else {
        error!("Render loop not yet initialized");
        return Err(Error::from_hresult(HRESULT(-1)));
    };

    let pipeline = Pipeline::new(hwnd, ctx, engine, render_loop).map_err(|(e, render_loop)| {
        RENDER_LOOP.get_or_init(move || render_loop);
        e
    })?;

    *ACTIVE_CONTEXT.lock() = Some(active_context);
    INIT_STATE.mark_done();

    Ok(Mutex::new(pipeline))
}

fn update_pipeline_display_size_from_swap_chain(
    pipeline: &mut Pipeline<D3D12RenderEngine>,
    swap_chain: &IDXGISwapChain3,
) -> Result<()> {
    let desc = unsafe { swap_chain.GetDesc() }?;
    pipeline.update_display_size_from_swap_chain(desc.BufferDesc.Width, desc.BufferDesc.Height);
    Ok(())
}

unsafe fn wait_for_pipeline_idle() -> Result<()> {
    if let Some(pipeline) = PIPELINE.get() {
        pipeline.lock().wait_idle()?;
    }

    Ok(())
}

unsafe fn wait_for_pipeline_idle_before(operation: &str) {
    if let Err(error) = wait_for_pipeline_idle() {
        util::print_dxgi_debug_messages();
        error!("Could not wait for DX12 pipeline to become idle before {operation}: {error:?}");
    }
}

unsafe fn update_display_size_after_swap_chain_change(
    swap_chain: &IDXGISwapChain3,
    operation: &str,
) {
    let Some(pipeline) = PIPELINE.get() else {
        return;
    };
    let Some(mut pipeline) = pipeline.try_lock() else {
        warn!("Could not lock DX12 pipeline to update display size after {operation}");
        return;
    };

    if let Err(error) = update_pipeline_display_size_from_swap_chain(&mut pipeline, swap_chain) {
        warn!("Could not update DX12 display size after {operation}: {error:?}");
    }
}

fn render(swap_chain: &IDXGISwapChain3) -> Result<()> {
    if !crate::jr_state::GATE.load(Ordering::SeqCst) {
        return Ok(());
    }
    unsafe {
        if !validate_pending_initialization_context(swap_chain)? {
            return Ok(());
        }
        if !validate_active_context(swap_chain)? {
            return Ok(());
        }

        let swap_chain_desc = swap_chain.GetDesc()?;
        if D3D12RenderEngine::rtv_format_for_swap_chain(swap_chain_desc.BufferDesc.Format).is_none()
        {
            warn!(
                "DX12 swap-chain format {:?} is not supported by hudhook; skipping overlay",
                swap_chain_desc.BufferDesc.Format,
            );
            return Ok(());
        }

        if PIPELINE.get().is_none() && INIT_STATE.lock().get().is_none() {
            return Ok(());
        }

        let pipeline = PIPELINE.get_or_try_init(|| init_pipeline())?;

        let Some(mut pipeline) = pipeline.try_lock() else {
            error!("Could not lock pipeline");
            return Err(Error::from_hresult(HRESULT(-1)));
        };

        if let Err(e) = update_pipeline_display_size_from_swap_chain(&mut pipeline, swap_chain) {
            warn!("Could not update DX12 display size from swap chain: {e:?}");
        }

        pipeline.prepare_render()?;

        if let Err(e) = update_pipeline_display_size_from_swap_chain(&mut pipeline, swap_chain) {
            warn!(
                "Could not update DX12 display size from swap chain after window messages: {e:?}"
            );
        }

        let target: ID3D12Resource =
            swap_chain.GetBuffer(swap_chain.GetCurrentBackBufferIndex())?;

        pipeline.render(target)?;
    }

    Ok(())
}

unsafe extern "system" fn dxgi_swap_chain_present_impl(
    swap_chain: IDXGISwapChain,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let swap_chain3 = swap_chain.cast::<IDXGISwapChain3>().ok();
    if let Some(swap_chain3) = &swap_chain3 {
        INIT_STATE.lock().insert_swap_chain(swap_chain3);
    }

    let Trampolines { dxgi_swap_chain_present, .. } =
        TRAMPOLINES.get().expect("DirectX 12 trampolines uninitialized");

    crate::jr_state::PRESENT_THREAD.store(windows::Win32::System::Threading::GetCurrentThreadId(), std::sync::atomic::Ordering::SeqCst);
    crate::jr_state::PRESENT_STAGE.store(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(swap_chain3) = &swap_chain3 {
        if let Err(e) = render(swap_chain3) {
            util::print_dxgi_debug_messages();
            error!("Render error: {e:?}");
        }
    }

    trace!("Call IDXGISwapChain::Present trampoline");
    crate::jr_state::PRESENT_STAGE.store(2, std::sync::atomic::Ordering::SeqCst);
    let result = dxgi_swap_chain_present(swap_chain, sync_interval, flags);
    crate::jr_state::PRESENT_STAGE.store(0, std::sync::atomic::Ordering::SeqCst);
    crate::jr_state::PRESENTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    handle_present_result("IDXGISwapChain::Present", result);

    if EJECT_REQUESTED.load(Ordering::SeqCst) {
        perform_eject();
    }

    result
}

unsafe extern "system" fn dxgi_swap_chain_present1_impl(
    swap_chain: IDXGISwapChain1,
    sync_interval: u32,
    present_flags: u32,
    present_parameters: *const DXGI_PRESENT_PARAMETERS,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let swap_chain3 = swap_chain.cast::<IDXGISwapChain3>().ok();
    if let Some(swap_chain3) = &swap_chain3 {
        INIT_STATE.lock().insert_swap_chain(swap_chain3);
    }

    let Trampolines { dxgi_swap_chain_present1, .. } =
        TRAMPOLINES.get().expect("DirectX 12 trampolines uninitialized");

    crate::jr_state::PRESENT_THREAD.store(windows::Win32::System::Threading::GetCurrentThreadId(), std::sync::atomic::Ordering::SeqCst);
    crate::jr_state::PRESENT_STAGE.store(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(swap_chain3) = &swap_chain3 {
        if let Err(e) = render(swap_chain3) {
            util::print_dxgi_debug_messages();
            error!("Render error: {e:?}");
        }
    }

    trace!("Call IDXGISwapChain::Present1 trampoline");
    crate::jr_state::PRESENT_STAGE.store(2, std::sync::atomic::Ordering::SeqCst);
    let result =
        dxgi_swap_chain_present1(swap_chain, sync_interval, present_flags, present_parameters);
    crate::jr_state::PRESENT_STAGE.store(0, std::sync::atomic::Ordering::SeqCst);
    crate::jr_state::PRESENTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    handle_present_result("IDXGISwapChain1::Present1", result);

    if EJECT_REQUESTED.load(Ordering::SeqCst) {
        perform_eject();
    }

    result
}

unsafe extern "system" fn dxgi_swap_chain_resize_buffers_impl(
    p_this: IDXGISwapChain,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: DXGI_FORMAT,
    flags: u32,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let Trampolines { dxgi_swap_chain_resize_buffers, .. } =
        TRAMPOLINES.get().expect("DirectX 12 trampolines uninitialized");

    wait_for_pipeline_idle_before("ResizeBuffers");

    let swap_chain3 = p_this.cast::<IDXGISwapChain3>().ok();

    trace!("Call IDXGISwapChain::ResizeBuffers trampoline");
    let result =
        dxgi_swap_chain_resize_buffers(p_this, buffer_count, width, height, new_format, flags);
    if result.is_err() {
        return result;
    }

    let Some(swap_chain3) = swap_chain3.as_ref() else {
        return result;
    };
    update_display_size_after_swap_chain_change(swap_chain3, "ResizeBuffers");

    result
}

unsafe extern "system" fn dxgi_swap_chain_set_source_size_impl(
    swap_chain: IDXGISwapChain2,
    width: u32,
    height: u32,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let Trampolines { dxgi_swap_chain_set_source_size, .. } =
        TRAMPOLINES.get().expect("DirectX 12 trampolines uninitialized");

    wait_for_pipeline_idle_before("SetSourceSize");

    trace!("Call IDXGISwapChain2::SetSourceSize trampoline");
    let result = dxgi_swap_chain_set_source_size(swap_chain.clone(), width, height);
    if result.is_err() {
        return result;
    }

    let Ok(swap_chain3) = swap_chain.cast::<IDXGISwapChain3>() else {
        return result;
    };
    update_display_size_after_swap_chain_change(&swap_chain3, "SetSourceSize");

    result
}

unsafe extern "system" fn dxgi_swap_chain_resize_buffers1_impl(
    p_this: IDXGISwapChain3,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: DXGI_FORMAT,
    flags: u32,
    creation_node_mask: *const u32,
    present_queue: *const Option<IUnknown>,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let Trampolines { dxgi_swap_chain_resize_buffers1, .. } =
        TRAMPOLINES.get().expect("DirectX 12 trampolines uninitialized");

    wait_for_pipeline_idle_before("ResizeBuffers1");

    let swap_chain = p_this.clone();
    let present_queue_count = if present_queue.is_null() {
        0
    } else if buffer_count != 0 {
        buffer_count
    } else {
        match swap_chain.GetDesc() {
            Ok(desc) => desc.BufferCount,
            Err(e) => {
                warn!("Could not query DX12 swap-chain buffer count before ResizeBuffers1: {e:?}");
                0
            },
        }
    };
    let command_queue =
        command_queue_from_present_queues(present_queue, creation_node_mask, present_queue_count);

    trace!("Call IDXGISwapChain3::ResizeBuffers1 trampoline");
    let result = dxgi_swap_chain_resize_buffers1(
        p_this,
        buffer_count,
        width,
        height,
        new_format,
        flags,
        creation_node_mask,
        present_queue,
    );

    if result.is_err() {
        return result;
    }

    match command_queue {
        PresentQueueCapture::Valid(command_queue) => {
            reset_pipeline("ResizeBuffers1 present queue update");
            complete_initialization_from_queue(&swap_chain, &command_queue);
        },
        PresentQueueCapture::Invalid => {
            reset_pipeline("ResizeBuffers1 unsupported present queue configuration");
        },
        PresentQueueCapture::Missing => {
            update_display_size_after_swap_chain_change(&swap_chain, "ResizeBuffers1");
        },
    }

    result
}

unsafe extern "system" fn d3d12_command_queue_execute_command_lists_impl(
    command_queue: ID3D12CommandQueue,
    num_command_lists: u32,
    command_lists: *mut ID3D12CommandList,
) {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();

    if !INIT_STATE.is_done() {
        trace!(
            "ID3D12CommandQueue::ExecuteCommandLists({command_queue:?}, {num_command_lists}, \
             {command_lists:p}) invoked",
        );

        {
            INIT_STATE.lock().insert_command_queue(&command_queue);
        }
    }

    let Trampolines { d3d12_command_queue_execute_command_lists, .. } =
        TRAMPOLINES.get().expect("DirectX 12 trampolines uninitialized");

    d3d12_command_queue_execute_command_lists(command_queue, num_command_lists, command_lists);
}

unsafe extern "system" fn dxgi_factory_create_swap_chain_impl(
    factory: IDXGIFactory,
    device: IUnknown,
    desc: *const DXGI_SWAP_CHAIN_DESC,
    swap_chain: *mut Option<IDXGISwapChain>,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let dxgi_factory_create_swap_chain =
        FACTORY.get().expect("DirectX 12 factory trampolines uninitialized").create_swap_chain;

    trace!("Call IDXGIFactory::CreateSwapChain trampoline");
    let result = dxgi_factory_create_swap_chain(factory, device.clone(), desc, swap_chain);

    let hwnd = if desc.is_null() { HWND::default() } else { (*desc).OutputWindow };
    let swap_chain3 = if swap_chain.is_null() {
        None
    } else {
        (*swap_chain).as_ref().and_then(|swap_chain| swap_chain.cast().ok())
    };
    handle_swap_chain_created(hwnd, &device, swap_chain3, result);

    result
}

unsafe extern "system" fn dxgi_factory_create_swap_chain_for_hwnd_impl(
    factory: IDXGIFactory2,
    device: IUnknown,
    hwnd: HWND,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen_desc: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    restrict_to_output: Option<IDXGIOutput>,
    swap_chain: *mut Option<IDXGISwapChain1>,
) -> HRESULT {
    let _hook_ejection_guard = HOOK_EJECTION_BARRIER.acquire_ejection_guard();
    let dxgi_factory_create_swap_chain_for_hwnd = FACTORY
        .get()
        .expect("DirectX 12 factory trampolines uninitialized")
        .create_swap_chain_for_hwnd;

    trace!("Call IDXGIFactory2::CreateSwapChainForHwnd trampoline");
    let result = dxgi_factory_create_swap_chain_for_hwnd(
        factory,
        device.clone(),
        hwnd,
        desc,
        fullscreen_desc,
        restrict_to_output,
        swap_chain,
    );

    let swap_chain3 = if swap_chain.is_null() {
        None
    } else {
        (*swap_chain).as_ref().and_then(|swap_chain| swap_chain.cast().ok())
    };
    handle_swap_chain_created(hwnd, &device, swap_chain3, result);

    result
}

fn get_target_addrs() -> (
    DXGIFactoryCreateSwapChainType,
    DXGIFactoryCreateSwapChainForHwndType,
    DXGISwapChainPresentType,
    DXGISwapChainPresent1Type,
    DXGISwapChainResizeBuffersType,
    DXGISwapChainSetSourceSizeType,
    DXGISwapChainResizeBuffers1Type,
    D3D12CommandQueueExecuteCommandListsType,
) {
    let dummy_hwnd = DummyHwnd::new();

    let factory: IDXGIFactory2 =
        unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
    let adapter = unsafe { factory.EnumAdapters(0) }.unwrap();

    let device: ID3D12Device =
        util::try_out_ptr(|v| unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, v) })
            .expect("D3D12CreateDevice failed");

    let command_queue: ID3D12CommandQueue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            Priority: 0,
            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
            NodeMask: 0,
        })
    }
    .unwrap();

    let swap_chain: IDXGISwapChain = match util::try_out_ptr(|v| unsafe {
        factory
            .CreateSwapChain(
                &command_queue,
                &DXGI_SWAP_CHAIN_DESC {
                    BufferDesc: DXGI_MODE_DESC {
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                        Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
                        Width: 640,
                        Height: 480,
                        RefreshRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    OutputWindow: dummy_hwnd.hwnd(),
                    Windowed: BOOL(1),
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as _,
                },
                v,
            )
            .ok()
    }) {
        Ok(swap_chain) => swap_chain,
        Err(e) => {
            util::print_dxgi_debug_messages();
            panic!("{e:?}");
        },
    };

    let swap_chain2: IDXGISwapChain2 = swap_chain.cast().unwrap();
    let swap_chain3: IDXGISwapChain3 = swap_chain.cast().unwrap();

    let create_swap_chain_ptr: DXGIFactoryCreateSwapChainType =
        unsafe { mem::transmute(factory.vtable().base__.base__.CreateSwapChain) };
    let create_swap_chain_for_hwnd_ptr: DXGIFactoryCreateSwapChainForHwndType =
        unsafe { mem::transmute(factory.vtable().CreateSwapChainForHwnd) };
    let present_ptr: DXGISwapChainPresentType =
        unsafe { mem::transmute(swap_chain.vtable().Present) };
    let present1_ptr: DXGISwapChainPresent1Type =
        unsafe { mem::transmute(swap_chain3.vtable().base__.base__.Present1) };
    let resize_buffers_ptr: DXGISwapChainResizeBuffersType =
        unsafe { mem::transmute(swap_chain.vtable().ResizeBuffers) };
    let set_source_size_ptr: DXGISwapChainSetSourceSizeType =
        unsafe { mem::transmute(swap_chain2.vtable().SetSourceSize) };
    let resize_buffers1_ptr: DXGISwapChainResizeBuffers1Type =
        unsafe { mem::transmute(swap_chain3.vtable().ResizeBuffers1) };
    let cqecl_ptr: D3D12CommandQueueExecuteCommandListsType =
        unsafe { mem::transmute(command_queue.vtable().ExecuteCommandLists) };

    (
        create_swap_chain_ptr,
        create_swap_chain_for_hwnd_ptr,
        present_ptr,
        present1_ptr,
        resize_buffers_ptr,
        set_source_size_ptr,
        resize_buffers1_ptr,
        cqecl_ptr,
    )
}

/// Hooks for DirectX 12.
pub struct ImguiDx12Hooks([MhHook; 2]);

/// (Elden Force 2.0.7) The two swap-chain makers, found from a DXGI factory alone (no device, queue, swap chain or window).
fn factory_addrs() -> (DXGIFactoryCreateSwapChainType, DXGIFactoryCreateSwapChainForHwndType) {
    let factory: IDXGIFactory2 =
        unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
    unsafe {
        (
            mem::transmute(factory.vtable().base__.base__.CreateSwapChain),
            mem::transmute(factory.vtable().CreateSwapChainForHwnd),
        )
    }
}

/// (Elden Force 2.0.7) Hook Present, Present1, ResizeBuffers, SetSourceSize, ResizeBuffers1 and ExecuteCommandLists at
/// these addresses, once. TRAMPOLINES is set before any of them is enabled.
unsafe fn late_hook(
    present: DXGISwapChainPresentType,
    present1: DXGISwapChainPresent1Type,
    resize_buffers: DXGISwapChainResizeBuffersType,
    set_source_size: DXGISwapChainSetSourceSizeType,
    resize_buffers1: DXGISwapChainResizeBuffers1Type,
    cqecl: D3D12CommandQueueExecuteCommandListsType,
    from: &str,
) {
    let _late = LATE_LOCK.lock();
    if LATE_DONE.load(Ordering::SeqCst) {
        return;
    }
    let Some(factory) = FACTORY.get().copied() else { return };
    let make = |addr: *mut c_void, imp: *mut c_void, what: &str| match MhHook::new(addr, imp) {
        Ok(h) => Some(h),
        Err(e) => {
            error!("couldn't create the {what} hook: {e:?}");
            None
        },
    };
    let (Some(h_present), Some(h_present1), Some(h_resize), Some(h_source), Some(h_resize1), Some(h_cqecl)) = (
        make(present as *mut _, dxgi_swap_chain_present_impl as *mut _, "Present"),
        make(present1 as *mut _, dxgi_swap_chain_present1_impl as *mut _, "Present1"),
        make(resize_buffers as *mut _, dxgi_swap_chain_resize_buffers_impl as *mut _, "ResizeBuffers"),
        make(set_source_size as *mut _, dxgi_swap_chain_set_source_size_impl as *mut _, "SetSourceSize"),
        make(resize_buffers1 as *mut _, dxgi_swap_chain_resize_buffers1_impl as *mut _, "ResizeBuffers1"),
        make(cqecl as *mut _, d3d12_command_queue_execute_command_lists_impl as *mut _, "ExecuteCommandLists"),
    ) else {
        return;
    };
    TRAMPOLINES.get_or_init(|| Trampolines {
        dxgi_factory_create_swap_chain: factory.create_swap_chain,
        dxgi_factory_create_swap_chain_for_hwnd: factory.create_swap_chain_for_hwnd,
        dxgi_swap_chain_present: mem::transmute::<*mut c_void, DXGISwapChainPresentType>(h_present.trampoline()),
        dxgi_swap_chain_present1: mem::transmute::<*mut c_void, DXGISwapChainPresent1Type>(h_present1.trampoline()),
        dxgi_swap_chain_resize_buffers: mem::transmute::<*mut c_void, DXGISwapChainResizeBuffersType>(
            h_resize.trampoline(),
        ),
        dxgi_swap_chain_set_source_size: mem::transmute::<*mut c_void, DXGISwapChainSetSourceSizeType>(
            h_source.trampoline(),
        ),
        dxgi_swap_chain_resize_buffers1: mem::transmute::<*mut c_void, DXGISwapChainResizeBuffers1Type>(
            h_resize1.trampoline(),
        ),
        d3d12_command_queue_execute_command_lists: mem::transmute::<
            *mut c_void,
            D3D12CommandQueueExecuteCommandListsType,
        >(h_cqecl.trampoline()),
    });
    for h in [&h_present, &h_present1, &h_resize, &h_source, &h_resize1, &h_cqecl] {
        if let Err(e) = h.queue_enable() {
            error!("couldn't queue a late hook: {e:?}");
        }
    }
    match crate::mh::MH_ApplyQueued().ok_context("MH_ApplyQueued") {
        Ok(()) => {
            crate::jr_state::LATE_HOOKED.store(true, Ordering::SeqCst);
            warn!("hooked from {from}");
        },
        Err(e) => error!("couldn't apply the late hooks: {e:?}"),
    }
    // (never unhooked: the game is never left with its Present unhooked mid-frame)
    mem::forget([h_present, h_present1, h_resize, h_source, h_resize1, h_cqecl]);
    LATE_DONE.store(true, Ordering::SeqCst);
}

/// (Elden Force 2.0.7) From the game's own swap chain and queue, as it makes them.
unsafe fn late_hook_from(swap_chain: &IDXGISwapChain3, command_queue: &ID3D12CommandQueue) {
    if LATE_DONE.load(Ordering::SeqCst) || crate::jr_state::NO_LATE.load(Ordering::SeqCst) {
        return;
    }
    let (Ok(sc), Ok(sc2)) = (swap_chain.cast::<IDXGISwapChain>(), swap_chain.cast::<IDXGISwapChain2>()) else {
        error!("the game's swap chain has no IDXGISwapChain/2");
        return;
    };
    late_hook(
        mem::transmute(sc.vtable().Present),
        mem::transmute(swap_chain.vtable().base__.base__.Present1),
        mem::transmute(sc.vtable().ResizeBuffers),
        mem::transmute(sc2.vtable().SetSourceSize),
        mem::transmute(swap_chain.vtable().ResizeBuffers1),
        mem::transmute(command_queue.vtable().ExecuteCommandLists),
        "the game's swap chain",
    );
}

/// (Elden Force 2.0.7) If the game made its swap chain before the factory was hooked (nothing hooked by the time the
/// overlay should start): hook as before, from a dummy device and swap chain. Present then finds the game's swap chain,
/// ExecuteCommandLists its queue.
pub fn late_hook_fallback() {
    if LATE_DONE.load(Ordering::SeqCst) {
        return;
    }
    let (_, _, present, present1, resize_buffers, set_source_size, resize_buffers1, cqecl) = get_target_addrs();
    unsafe {
        late_hook(present, present1, resize_buffers, set_source_size, resize_buffers1, cqecl, "a dummy swap chain (fallback)")
    };
}

impl ImguiDx12Hooks {
    /// Construct a set of [`MhHook`]s that will render UI via the
    /// provided [`ImguiRenderLoop`].
    ///
    /// Hooked at once: `IDXGIFactory::CreateSwapChain`, `IDXGIFactory2::CreateSwapChainForHwnd`. When the game makes its
    /// swap chain: `Present`, `Present1`, `ResizeBuffers`, `SetSourceSize`, `ResizeBuffers1` and the queue's
    /// `ExecuteCommandLists` (Elden Force 2.0.7).
    ///
    /// # Safety
    ///
    /// yolo
    pub unsafe fn new<T>(t: T) -> Self
    where
        T: ImguiRenderLoop + Send + Sync + 'static,
    {
        let (create_swap_chain_addr, create_swap_chain_for_hwnd_addr) = factory_addrs();

        let hook_create_swap_chain = MhHook::new(
            create_swap_chain_addr as *mut _,
            dxgi_factory_create_swap_chain_impl as *mut _,
        )
        .expect("couldn't create IDXGIFactory::CreateSwapChain hook");
        let hook_create_swap_chain_for_hwnd = MhHook::new(
            create_swap_chain_for_hwnd_addr as *mut _,
            dxgi_factory_create_swap_chain_for_hwnd_impl as *mut _,
        )
        .expect("couldn't create IDXGIFactory2::CreateSwapChainForHwnd hook");

        RENDER_LOOP.get_or_init(|| Box::new(t));

        FACTORY.get_or_init(|| FactoryTrampolines {
            create_swap_chain: mem::transmute::<*mut c_void, DXGIFactoryCreateSwapChainType>(
                hook_create_swap_chain.trampoline(),
            ),
            create_swap_chain_for_hwnd: mem::transmute::<*mut c_void, DXGIFactoryCreateSwapChainForHwndType>(
                hook_create_swap_chain_for_hwnd.trampoline(),
            ),
        });

        Self([hook_create_swap_chain, hook_create_swap_chain_for_hwnd])
    }
}

impl Hooks for ImguiDx12Hooks {
    fn from_render_loop<T>(t: T) -> Box<Self>
    where
        Self: Sized,
        T: ImguiRenderLoop + Send + Sync + 'static,
    {
        Box::new(unsafe { Self::new(t) })
    }

    fn hooks(&self) -> &[MhHook] {
        &self.0
    }

    unsafe fn unhook(&mut self) {
        TRAMPOLINES.take();
        PIPELINE.take().map(|p| p.into_inner().take());
        RENDER_LOOP.take();

        *ACTIVE_CONTEXT.lock() = None;
        INIT_STATE.reset();
    }
}
