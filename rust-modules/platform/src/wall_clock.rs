//! Local calendar values for presentation adapters; frame/replay clocks remain separate.
pub struct Reading {pub hour:u8,pub minute:u8,pub year:i32,pub month:u8,pub day:u8}
pub fn read()->Option<Reading>{
    let seconds=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let seconds:libc::time_t=seconds.try_into().ok()?;
    let mut local=std::mem::MaybeUninit::<libc::tm>::uninit();
    if unsafe{libc::localtime_r(&seconds,local.as_mut_ptr())}.is_null(){return None;}
    let local=unsafe{local.assume_init()};
    Some(Reading{hour:local.tm_hour.try_into().ok()?,minute:local.tm_min.try_into().ok()?,year:local.tm_year+1900,month:(local.tm_mon+1).try_into().ok()?,day:local.tm_mday.try_into().ok()?})
}
