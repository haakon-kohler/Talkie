use cpal::traits::{DeviceTrait, HostTrait};

pub struct CpalDeviceInfo {
    /// The setting matches on this; the picker lists it.
    pub name: String,
    /// The picker marks it; the capture path ignores it and asks cpal.
    pub is_default: bool,
    pub device: cpal::Device,
}

pub fn list_input_devices() -> Result<Vec<CpalDeviceInfo>, Box<dyn std::error::Error>> {
    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut out = Vec::<CpalDeviceInfo>::new();

    for device in host.input_devices()? {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());
        let is_default = Some(name.clone()) == default_name;

        out.push(CpalDeviceInfo {
            name,
            is_default,
            device,
        });
    }

    Ok(out)
}
