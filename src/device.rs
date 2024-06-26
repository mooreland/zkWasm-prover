pub mod cuda;

#[derive(Debug)]
pub enum Error {
    DeviceError(String),
    MsmError,
}

pub type DeviceResult<T> = Result<T, Error>;

pub trait DeviceBuf {}

pub trait Device<B: DeviceBuf>: Sized {
    fn get_device_count() -> DeviceResult<usize>;
    fn get_device(idx: usize) -> DeviceResult<Self>;

    fn alloc_device_buffer<T>(&self, size: usize) -> DeviceResult<B>;
    fn alloc_device_buffer_from_slice<T>(&self, data: &[T]) -> DeviceResult<B> {
        let buf = self.alloc_device_buffer::<T>(data.len())?;
        self.copy_from_host_to_device(&buf, data)?;
        Ok(buf)
    }

    fn copy_from_host_to_device<T>(&self, dst: &B, src: &[T]) -> DeviceResult<()>;
    fn copy_from_device_to_host<T>(&self, dst: &mut [T], src: &B) -> DeviceResult<()>;
    fn copy_from_device_to_device<T>(
        &self,
        dst: &B,
        dst_offset: usize,
        src: &B,
        src_offset: usize,
        len: usize,
    ) -> DeviceResult<()>;

    fn synchronize(&self) -> DeviceResult<()>;

    fn pin_memory<T>(&self, dst: &[T]) -> DeviceResult<()>;
    fn unpin_memory<T>(&self, dst: &[T]) -> DeviceResult<()>;
    
    fn print_memory_info(&self) -> DeviceResult<()>;
}
