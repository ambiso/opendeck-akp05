use mirajazz::{
    device::DeviceQuery,
    types::{HidDeviceInfo, ImageFormat, ImageMirroring, ImageMode, ImageRotation},
};

// Must be unique between all the plugins, 2 characters long and match `DeviceNamespace` field in `manifest.json`
pub const DEVICE_NAMESPACE: &str = "n4";

pub const ROW_COUNT: usize = 2;
pub const COL_COUNT: usize = 5;
pub const KEY_COUNT: usize = 15;
pub const ENCODER_COUNT: usize = 4;
pub const DEVICE_TYPE: u8 = 7; // StreamDeckPlus

#[derive(Debug, Clone)]
pub enum Kind {
    Akp05E,
    Akp05EPro,
    N4E,
    N4,
    N4ProE,
    N4Pro,
    VsdN4Pro,
    MsdPro,
    Cn003,
}

pub const VSDINSIDE_VID: u16 = 0x5548;
pub const VSD_N4_PRO_PID: u16 = 0x1023;

pub const AJAZZ_VID: u16 = 0x0300;
pub const AKP05E_PID: u16 = 0x3004;
pub const AKP05E_PRO_PID: u16 = 0x3013;

pub const MIRABOX_N4E_VID: u16 = 0x6603;
pub const N4E_PID: u16 = 0x1007;

pub const MIRABOX_N4_VID: u16 = 0x6602;
pub const N4_PID: u16 = 0x1001;

pub const MIRABOX_N4_PRO_E_VID: u16 = 0x5548;
pub const N4_PRO_E_PID: u16 = 0x1021;

pub const MIRABOX_N4_PRO_VID: u16 = 0x5548;
pub const N4_PRO_PID: u16 = 0x1008;

pub const MARS_GAMING_VID: u16 = 0x0B00;
pub const MSD_PRO_PID: u16 = 0x1003;

pub const SOOMFON_VID: u16 = 0x1500;
pub const CN003_PID: u16 = 0x3002;

// Map all queries to usage page 65440 and usage id 1 for now
pub const AKP05E_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, AJAZZ_VID, AKP05E_PID);
pub const AKP05E_PRO_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, AJAZZ_VID, AKP05E_PRO_PID);
pub const N4E_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, MIRABOX_N4E_VID, N4E_PID);
pub const N4_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, MIRABOX_N4_VID, N4_PID);
pub const N4_PRO_E_QUERY: DeviceQuery =
    DeviceQuery::new(65440, 1, MIRABOX_N4_PRO_E_VID, N4_PRO_E_PID);
pub const N4_PRO_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, MIRABOX_N4_PRO_VID, N4_PRO_PID);
pub const VSD_N4_PRO_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, VSDINSIDE_VID, VSD_N4_PRO_PID);
pub const MSD_PRO_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, MARS_GAMING_VID, MSD_PRO_PID);
pub const CN003_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, SOOMFON_VID, CN003_PID);

pub const QUERIES: &[DeviceQuery] = &[
    AKP05E_QUERY,
    AKP05E_PRO_QUERY,
    N4E_QUERY,
    N4_QUERY,
    N4_PRO_E_QUERY,
    N4_PRO_QUERY,
    VSD_N4_PRO_QUERY,
    MSD_PRO_QUERY,
    CN003_QUERY,
];

impl Kind {
    /// Matches devices VID+PID pairs to correct kinds
    pub fn from_vid_pid(vid: u16, pid: u16) -> Option<Self> {
        const { assert!(MIRABOX_N4_PRO_VID == VSDINSIDE_VID) };
        const { assert!(MIRABOX_N4_PRO_VID == MIRABOX_N4_PRO_E_VID) };
        match vid {
            AJAZZ_VID => match pid {
                AKP05E_PID => Some(Kind::Akp05E),
                AKP05E_PRO_PID => Some(Kind::Akp05EPro),
                _ => None,
            },

            MIRABOX_N4E_VID => match pid {
                N4E_PID => Some(Kind::N4E),
                _ => None,
            },

            MIRABOX_N4_VID => match pid {
                N4_PID => Some(Kind::N4),
                _ => None,
            },

            MIRABOX_N4_PRO_VID => match pid {
                VSD_N4_PRO_PID => Some(Kind::VsdN4Pro),
                N4_PRO_PID => Some(Kind::N4Pro),
                N4_PRO_E_PID => Some(Kind::N4ProE),
                _ => None,
            },

            MARS_GAMING_VID => match pid {
                MSD_PRO_PID => Some(Kind::MsdPro),
                _ => None,
            },

            SOOMFON_VID => match pid {
                CN003_PID => Some(Kind::Cn003),
                _ => None,
            },

            _ => None,
        }
    }

    /// There is no point relying on manufacturer/device names reported by the USB stack,
    /// so we return custom names for all the kinds of devices
    pub fn human_name(&self) -> String {
        match &self {
            // Ajazz devices
            Self::Akp05E => "Ajazz AKP05E",
            Self::Akp05EPro => "Ajazz AKP05E Pro",
            // Mirabox devices
            Self::N4 => "Mirabox N4",
            Self::N4E => "Mirabox N4E",
            Self::N4ProE => "Mirabox N4 Pro E",
            Self::N4Pro => "Mirabox N4 Pro",
            // VSDInside devices
            Self::VsdN4Pro => "VSDInside N4 Pro",
            // Mars Gaming devices
            Self::MsdPro => "Mars Gaming MSD-Pro",
            // Soomfon devices
            Self::Cn003 => "Soomfon CN003",
        }
        .to_string()
    }

    /// Returns protocol version for device
    pub fn protocol_version(&self) -> usize {
        match self {
            _ => 3,
        }
    }

    pub fn image_format(&self) -> ImageFormat {
        if self.protocol_version() == 3 {
            return ImageFormat {
                mode: ImageMode::JPEG,
                size: (112, 112),
                rotation: ImageRotation::Rot180,
                mirror: ImageMirroring::None,
            };
        }

        return ImageFormat {
            mode: ImageMode::JPEG,
            size: (60, 60),
            rotation: ImageRotation::Rot0,
            mirror: ImageMirroring::None,
        };
    }
    pub fn touch_image_format(&self) -> ImageFormat {
        if self.protocol_version() == 3 {
            return ImageFormat {
                mode: ImageMode::JPEG,
                size: (176, 112), // from https://github.com/MiraboxSpace/StreamDock-Device-SDK/blob/31d887551de556bd0776bf4982233999d58e49d1/CPP-SDK/src/HotspotDevice/StreamDockN4/streamdockN4.cpp#L57
                rotation: ImageRotation::Rot180,
                mirror: ImageMirroring::None,
            };
        }

        return ImageFormat {
            mode: ImageMode::JPEG,
            size: (60, 60),
            rotation: ImageRotation::Rot0,
            mirror: ImageMirroring::None,
        };
    }

    pub fn supports_both_encoder_states(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct CandidateDevice {
    pub id: String,
    pub dev: HidDeviceInfo,
    pub kind: Kind,
}
