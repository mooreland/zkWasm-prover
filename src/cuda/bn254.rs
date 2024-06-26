use super::bn254_c;
use crate::device::cuda::{to_result, CudaBuffer, CudaDevice, CudaDeviceBufRaw};
use crate::device::Error;
use crate::device::{Device, DeviceResult};

use core::mem::ManuallyDrop;
use cuda_runtime_sys::{cudaDeviceSynchronize, cudaStream_t};
use halo2_proofs::arithmetic::{CurveAffine, FieldExt};
use icicle_bn254::curve::BaseField;
use icicle_bn254::curve::CurveCfg;
use icicle_bn254::curve::G1Projective;
use icicle_core::curve::Projective;
use icicle_core::msm;
use icicle_core::traits::FieldImpl;
use icicle_cuda_runtime::memory::HostOrDeviceSlice;
use icicle_cuda_runtime::stream::CudaStream;

pub(crate) fn extended_prepare(
    device: &CudaDevice,
    s: &CudaDeviceBufRaw,
    coset_powers: &CudaDeviceBufRaw,
    coset_powers_n: usize,
    size: usize,
    extended_size: usize,
    stream: Option<cudaStream_t>,
) -> Result<(), Error> {
    unsafe {
        device.acitve_ctx()?;
        let err = bn254_c::extended_prepare(
            s.ptr(),
            coset_powers.ptr(),
            coset_powers_n as i32,
            size as i32,
            extended_size as i32,
            0,
            stream.unwrap_or(0usize as _),
        );
        to_result((), err, "fail to run extended_prepare")?;
        Ok(())
    }
}

pub(crate) fn extended_intt_after(
    device: &CudaDevice,
    s: &CudaDeviceBufRaw,
    coset_powers: &CudaDeviceBufRaw,
    coset_powers_n: usize,
    size: usize,
    extended_size: usize,
    stream: Option<cudaStream_t>,
) -> Result<(), Error> {
    unsafe {
        device.acitve_ctx()?;
        let err = bn254_c::extended_prepare(
            s.ptr(),
            coset_powers.ptr(),
            coset_powers_n as i32,
            size as i32,
            extended_size as i32,
            1,
            stream.unwrap_or(0usize as _),
        );
        to_result((), err, "fail to run extended_prepare")?;
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum FieldOp {
    Add = 0,
    Mul = 1,
    UOp = 2,
    Sub = 3,
}

pub(crate) fn field_op_v2<F: FieldExt>(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    l: Option<&CudaDeviceBufRaw>,
    l_c: Option<F>,
    r: Option<&CudaDeviceBufRaw>,
    r_c: Option<F>,
    size: usize,
    op: FieldOp,
) -> Result<(), Error> {
    field_op(device, res, l, 0, l_c, r, 0, r_c, size, op, None)?;

    Ok(())
}

pub(crate) fn field_sub<F: FieldExt>(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    rhs: &CudaDeviceBufRaw,
    size: usize,
) -> Result<(), Error> {
    field_op_v2::<F>(
        device,
        res,
        Some(res),
        None,
        Some(rhs),
        None,
        size,
        FieldOp::Sub,
    )?;
    Ok(())
}

pub(crate) fn field_mul<F: FieldExt>(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    rhs: &CudaDeviceBufRaw,
    size: usize,
) -> Result<(), Error> {
    field_op_v2::<F>(
        device,
        res,
        Some(res),
        None,
        Some(rhs),
        None,
        size,
        FieldOp::Mul,
    )?;
    Ok(())
}

pub(crate) fn pick_from_buf<F: FieldExt>(
    device: &CudaDevice,
    buf: &CudaDeviceBufRaw,
    rot: isize,
    i: isize,
    size: usize,
) -> Result<F, Error> {
    let mut v = [F::zero()];
    device.acitve_ctx()?;
    unsafe {
        let err = cuda_runtime_sys::cudaMemcpy(
            v.as_mut_ptr() as _,
            buf.ptr().offset(
                ((rot + i + size as isize) & (size as isize - 1))
                    * core::mem::size_of::<F>() as isize,
            ),
            core::mem::size_of::<F>(),
            cuda_runtime_sys::cudaMemcpyKind::cudaMemcpyDeviceToHost,
        );
        to_result((), err, "fail to pick_from_buf")?;
    }
    Ok(v[0])
}

pub(crate) fn field_op_v3(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    l: Option<&CudaDeviceBufRaw>,
    l_c: Option<&CudaDeviceBufRaw>,
    r: Option<&CudaDeviceBufRaw>,
    r_c: Option<&CudaDeviceBufRaw>,
    size: usize,
    op: FieldOp,
    stream: Option<cudaStream_t>,
) -> Result<(), Error> {
    unsafe {
        device.acitve_ctx()?;
        let err = bn254_c::field_op(
            res.ptr(),
            l.map_or(0usize as *mut _, |x| x.ptr()),
            0,
            l_c.as_ref().map_or(0usize as *mut _, |x| x.ptr()),
            r.map_or(0usize as *mut _, |x| x.ptr()),
            0,
            r_c.as_ref().map_or(0usize as *mut _, |x| x.ptr()),
            size as i32,
            op as i32,
            stream.unwrap_or(0usize as _),
        );
        to_result((), err, "fail to run field_op")?;
    }
    Ok(())
}

pub(crate) fn field_op<F: FieldExt>(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    l: Option<&CudaDeviceBufRaw>,
    l_rot: i32,
    l_c: Option<F>,
    r: Option<&CudaDeviceBufRaw>,
    r_rot: i32,
    r_c: Option<F>,
    size: usize,
    op: FieldOp,
    stream: Option<cudaStream_t>,
) -> Result<(), Error> {
    let l_c = if l_c.is_none() {
        None
    } else {
        Some(device.alloc_device_buffer_from_slice([l_c.unwrap()].as_slice())?)
    };
    let r_c = if r_c.is_none() {
        None
    } else {
        Some(device.alloc_device_buffer_from_slice([r_c.unwrap()].as_slice())?)
    };

    unsafe {
        device.acitve_ctx()?;
        let err = bn254_c::field_op(
            res.ptr(),
            l.map_or(0usize as *mut _, |x| x.ptr()),
            l_rot,
            l_c.as_ref().map_or(0usize as *mut _, |x| x.ptr()),
            r.map_or(0usize as *mut _, |x| x.ptr()),
            r_rot,
            r_c.as_ref().map_or(0usize as *mut _, |x| x.ptr()),
            size as i32,
            op as i32,
            stream.unwrap_or(0usize as _),
        );
        to_result((), err, "fail to run field_op")?;
    }
    Ok(())
}

pub fn msm_single_buffer<C: CurveAffine>(
    p_buf: &CudaDeviceBufRaw,
    s_buf: &CudaDeviceBufRaw,
    len: usize,
) -> DeviceResult<C> {
    for _ in 0..100 {
        let res = msm_single_buffer_core(p_buf, s_buf, len);

        if res.is_ok() {
            return res;
        }
    }

    unreachable!()
}

fn msm_single_buffer_core<C: CurveAffine>(
    p_buf: &CudaDeviceBufRaw,
    s_buf: &CudaDeviceBufRaw,
    len: usize,
) -> DeviceResult<C> {
    unsafe {
        // Ensure s_buf and p_buf are ready
        cudaDeviceSynchronize();
    }

    let mut msm_results = HostOrDeviceSlice::cuda_malloc(1).unwrap();

    let points = {
        unsafe {
            ManuallyDrop::new(HostOrDeviceSlice::Device(
                std::slice::from_raw_parts_mut(p_buf.ptr() as _, len),
                0,
            ))
        }
    };

    let scalars = {
        unsafe {
            ManuallyDrop::new(HostOrDeviceSlice::Device(
                std::slice::from_raw_parts_mut(s_buf.ptr() as _, len),
                0,
            ))
        }
    };
    //Use async would cause failure on multi-open;
    //scalars.copy_from_host_async(value, &stream).unwrap();
    let mut cfg = msm::MSMConfig::default();
    cfg.are_scalars_montgomery_form = true;
    cfg.are_points_montgomery_form = true;
    msm::msm(&scalars, &points, &cfg, &mut msm_results).unwrap();

    let res = copy_and_to_affine(&msm_results)?;

    Ok(res)
}

pub fn batch_msm<C: CurveAffine>(
    p_buf: &CudaDeviceBufRaw,
    s_buf: [&CudaDeviceBufRaw; 2],
    values: Vec<&[C::Scalar]>,
    len: usize,
) -> Result<Vec<C>, Error> {
    for _ in 0..100 {
        let res = batch_msm_core(p_buf, s_buf, values.clone(), len);

        if res.is_ok() {
            return res;
        }
    }

    unreachable!()
}

fn batch_msm_core<C: CurveAffine>(
    p_buf: &CudaDeviceBufRaw,
    s_buf: [&CudaDeviceBufRaw; 2],
    values: Vec<&[C::Scalar]>,
    len: usize,
) -> Result<Vec<C>, Error> {
    unsafe {
        // Ensure s_buf and p_buf are ready
        cudaDeviceSynchronize();
    }

    let mut res_vec = vec![];
    let mut last_stream: Option<CudaStream> = None;
    let mut msm_results = [
        HostOrDeviceSlice::cuda_malloc(1).unwrap(),
        HostOrDeviceSlice::cuda_malloc(1).unwrap(),
    ];

    let points = {
        unsafe {
            ManuallyDrop::new(HostOrDeviceSlice::Device(
                std::slice::from_raw_parts_mut(p_buf.ptr() as _, len),
                0,
            ))
        }
    };

    let msm_counts = values.len();

    for (idx, value) in values.iter().enumerate() {
        let mut scalars = {
            unsafe {
                ManuallyDrop::new(HostOrDeviceSlice::Device(
                    std::slice::from_raw_parts_mut(s_buf[idx & 1].ptr() as _, len),
                    0,
                ))
            }
        };
        let stream = CudaStream::create().unwrap();
        let value = unsafe { core::mem::transmute::<_, _>(*value) };
        //Use async would cause failure on multi-open;
        //scalars.copy_from_host_async(value, &stream).unwrap();
        scalars.copy_from_host(value).unwrap();
        let mut cfg = msm::MSMConfig::default();
        cfg.ctx.stream = &stream;
        cfg.is_async = true;
        cfg.are_scalars_montgomery_form = true;
        cfg.are_points_montgomery_form = true;
        msm::msm(&scalars, &points, &cfg, &mut msm_results[idx & 1]).unwrap();

        if let Some(last_stream) = last_stream {
            let last_idx = 1 - (idx & 1);
            last_stream.synchronize().unwrap();
            let res = copy_and_to_affine(&msm_results[last_idx])?;

            res_vec.push(res);
        }
        last_stream = Some(stream);
    }

    if let Some(last_stream) = last_stream {
        let last_idx = 1 - (msm_counts & 1);
        last_stream.synchronize().unwrap();
        let res = copy_and_to_affine(&msm_results[last_idx])?;
        res_vec.push(res);
    }

    Ok(res_vec)
}

fn copy_and_to_affine<C: CurveAffine>(
    msm_result: &HostOrDeviceSlice<'_, Projective<CurveCfg>>,
) -> DeviceResult<C> {
    let retry_limit = 3;

    for i in 0..retry_limit {
        let mut msm_host_result = [G1Projective::zero()];
        msm_result.copy_to_host(&mut msm_host_result[..]).unwrap();
        let res = to_affine(&msm_host_result[0]);
        if res.is_some() {
            return Ok(res.unwrap());
        }

        println!("bad msm result at round {} is {:?}", i, msm_host_result);
    }

    Err(Error::MsmError)
}

// msm sometimes return bad point, retry to make it correct
fn to_affine<C: CurveAffine>(g: &icicle_bn254::curve::G1Projective) -> Option<C> {
    if g.z == BaseField::zero() {
        Some(C::identity())
    } else {
        use halo2_proofs::arithmetic::BaseExt;
        use halo2_proofs::arithmetic::Field;

        let mut t: Vec<_> = g.x.to_bytes_le();
        t.resize(64, 0u8);
        let x = C::Base::from_bytes_wide(&t.try_into().unwrap());

        let mut t: Vec<_> = g.y.to_bytes_le();
        t.resize(64, 0u8);
        let y = C::Base::from_bytes_wide(&t.try_into().unwrap());

        let mut t: Vec<_> = g.z.to_bytes_le();
        t.resize(64, 0u8);
        let z = C::Base::from_bytes_wide(&t.try_into().unwrap());

        let z_inv = z.invert().unwrap();
        C::from_xy(x * z_inv, y * z_inv).into()
    }
}

pub const MAX_DEG: usize = 8;

pub fn ntt_prepare<F: FieldExt>(
    device: &CudaDevice,
    omega: F,
    len_log: usize,
) -> DeviceResult<(CudaDeviceBufRaw, CudaDeviceBufRaw)> {
    let len = 1 << len_log;
    let omegas = vec![F::one(), omega];

    let max_deg = MAX_DEG.min(len_log);
    let mut pq = vec![F::zero(); 1 << max_deg >> 1];
    let twiddle = omega.pow_vartime([(len >> max_deg) as u64]);
    pq[0] = F::one();
    if max_deg > 1 {
        pq[1] = twiddle;
        for i in 2..(1 << max_deg >> 1) {
            pq[i] = pq[i - 1];
            pq[i].mul_assign(&twiddle);
        }
    }

    let omegas_buf = device.alloc_device_buffer::<F>(1 << len_log)?;
    device.copy_from_host_to_device(&omegas_buf, &omegas[..])?;
    unsafe {
        let err =
            crate::cuda::bn254_c::expand_omega_buffer(omegas_buf.ptr(), (1 << len_log) as i32);
        to_result((), err, "fail to run expand_omega_buffer")?;
    }
    let pq_buf = device.alloc_device_buffer_from_slice(&pq[..])?;

    Ok((omegas_buf, pq_buf))
}

pub fn ntt_raw(
    device: &CudaDevice,
    s_buf: &mut CudaDeviceBufRaw,
    tmp_buf: &mut CudaDeviceBufRaw,
    pq_buf: &CudaDeviceBufRaw,
    omegas_buf: &CudaDeviceBufRaw,
    len_log: usize,
    stream: Option<cudaStream_t>,
) -> Result<(), Error> {
    let mut swap = false;
    unsafe {
        device.acitve_ctx()?;
        let err = crate::cuda::bn254::bn254_c::ntt(
            s_buf.ptr(),
            tmp_buf.ptr(),
            pq_buf.ptr(),
            omegas_buf.ptr(),
            len_log as i32,
            MAX_DEG as i32,
            &mut swap as *mut _ as _,
            stream.unwrap_or(0usize as _),
        );
        to_result((), err, "fail to run ntt")?;
    }
    if swap {
        std::mem::swap(s_buf, tmp_buf);
    }
    Ok(())
}

pub fn intt_raw(
    device: &CudaDevice,
    s_buf: &mut CudaDeviceBufRaw,
    tmp_buf: &mut CudaDeviceBufRaw,
    pq_buf: &CudaDeviceBufRaw,
    omegas_buf: &CudaDeviceBufRaw,
    divisor: &CudaDeviceBufRaw,
    len_log: usize,
) -> Result<(), Error> {
    intt_raw_async(
        device, s_buf, tmp_buf, pq_buf, omegas_buf, divisor, len_log, None,
    )
}

pub fn intt_raw_async(
    device: &CudaDevice,
    s_buf: &mut CudaDeviceBufRaw,
    tmp_buf: &mut CudaDeviceBufRaw,
    pq_buf: &CudaDeviceBufRaw,
    omegas_buf: &CudaDeviceBufRaw,
    divisor: &CudaDeviceBufRaw,
    len_log: usize,
    stream: Option<cudaStream_t>,
) -> Result<(), Error> {
    ntt_raw(device, s_buf, tmp_buf, pq_buf, omegas_buf, len_log, stream)?;
    unsafe {
        let err = bn254_c::field_op(
            s_buf.ptr(),
            s_buf.ptr(),
            0,
            0usize as *mut _,
            0usize as *mut _,
            0,
            divisor.ptr(),
            (1 << len_log) as i32,
            FieldOp::Mul as i32,
            stream.unwrap_or(0usize as _),
        );
        to_result((), err, "fail to run field_op in intt_raw")?;
    }
    Ok(())
}

pub fn ntt<F: FieldExt>(
    device: &CudaDevice,
    s_buf: &mut CudaDeviceBufRaw,
    tmp_buf: &mut CudaDeviceBufRaw,
    pq_buf: &CudaDeviceBufRaw,
    omegas_buf: &CudaDeviceBufRaw,
    result: &mut [F],
    len_log: usize,
) -> Result<(), Error> {
    ntt_raw(device, s_buf, tmp_buf, pq_buf, omegas_buf, len_log, None)?;
    device.copy_from_device_to_host(result, s_buf)?;
    Ok(())
}

// plonk permutation
pub fn permutation_eval_h_p1(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    first_set: &CudaDeviceBufRaw,
    last_set: &CudaDeviceBufRaw,
    l0: &CudaDeviceBufRaw,
    l_last: &CudaDeviceBufRaw,
    y: &CudaDeviceBufRaw,
    n: usize,
) -> Result<(), Error> {
    unsafe {
        device.acitve_ctx()?;
        let err = bn254_c::permutation_eval_h_p1(
            res.ptr(),
            first_set.ptr(),
            last_set.ptr(),
            l0.ptr(),
            l_last.ptr(),
            y.ptr(),
            n as i32,
        );
        to_result((), err, "fail to run permutation_eval_h_p1")?;
        device.synchronize()?;
    }
    Ok(())
}

pub fn permutation_eval_h_p2(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    set: &[CudaDeviceBufRaw],
    l0: &CudaDeviceBufRaw,
    l_last: &CudaDeviceBufRaw,
    y: &CudaDeviceBufRaw,
    rot: usize,
    n: usize,
) -> Result<(), Error> {
    unsafe {
        device.acitve_ctx()?;
        let sets = device
            .alloc_device_buffer_from_slice(&set.iter().map(|x| x.ptr()).collect::<Vec<_>>()[..])?;
        let err = bn254_c::permutation_eval_h_p2(
            res.ptr(),
            sets.ptr(),
            l0.ptr(),
            l_last.ptr(),
            y.ptr(),
            set.len() as i32,
            rot as i32,
            n as i32,
        );
        to_result((), err, "fail to run permutation_eval_h_p2")?;
        device.synchronize()?;
    }
    Ok(())
}

pub fn permutation_eval_h_l(
    device: &CudaDevice,
    res: &CudaDeviceBufRaw,
    beta: &CudaDeviceBufRaw,
    gamma: &CudaDeviceBufRaw,
    p: &CudaDeviceBufRaw,
    n: usize,
) -> Result<(), Error> {
    unsafe {
        device.acitve_ctx()?;
        let err =
            bn254_c::permutation_eval_h_l(res.ptr(), beta.ptr(), gamma.ptr(), p.ptr(), n as i32);
        to_result((), err, "fail to run permutation_eval_h_l")?;
        device.synchronize()?;
    }
    Ok(())
}

pub fn buffer_copy_with_shift<F: FieldExt>(
    device: &CudaDevice,
    dst: &CudaDeviceBufRaw,
    src: &CudaDeviceBufRaw,
    rot: isize,
    size: usize,
) -> Result<(), Error> {
    if rot == 0 {
        device.copy_from_device_to_device::<F>(&dst, 0, src, 0, size)?;
        device.synchronize()?;
    } else if rot > 0 {
        let rot = rot as usize;
        let len = size - rot as usize;
        device.copy_from_device_to_device::<F>(&dst, 0, src, rot, len)?;
        device.synchronize()?;
        device.copy_from_device_to_device::<F>(&dst, len, src, 0, rot)?;
        device.synchronize()?;
    } else {
        let rot = -rot as usize;
        let len = size - rot;
        device.copy_from_device_to_device::<F>(&dst, 0, src, rot, len)?;
        device.synchronize()?;
        device.copy_from_device_to_device::<F>(&dst, len, src, 0, rot)?;
        device.synchronize()?;
    }
    Ok(())
}
