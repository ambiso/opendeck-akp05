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
    N4EN,
    N4Pro,
    MSDPro,
}

pub const VSDINSIDE_VID: u16 = 0x5548;
pub const N4_PRO_PID: u16 = 0x1023;

pub const AJAZZ_VID: u16 = 0x0300;
pub const AKP05E_PID: u16 = 0x3004;

pub const MIRABOX_VID: u16 = 0x6603;
pub const N4EN_PID: u16 = 0x1007;

pub const MSDPRO_VID: u16 = 0x0B00;
pub const MSDPRO_PID: u16 = 0x1003;


// Map all queries to usage page 65440 and usage id 1 for now
pub const AKP05E_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, AJAZZ_VID, AKP05E_PID);
pub const N4EN_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, MIRABOX_VID, N4EN_PID);
pub const N4_PRO_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, VSDINSIDE_VID, N4_PRO_PID);
pub const MSDPRO_QUERY: DeviceQuery = DeviceQuery::new(65440, 1, MSDPRO_VID, MSDPRO_PID);



pub const QUERIES: &[DeviceQuery] = &[
    AKP05E_QUERY,
    N4EN_QUERY,
    N4_PRO_QUERY,
    MSDPRO_QUERY,
];


impl Kind {
    /// Matches devices VID+PID pairs to correct kinds
    pub fn from_vid_pid(vid: u16, pid: u16) -> Option<Self> {
        match vid {
            AJAZZ_VID => match pid {
                AKP05E_PID => Some(Kind::Akp05E),
                _ => None,
            },

            MIRABOX_VID => match pid {
                N4EN_PID => Some(Kind::N4EN),
                _ => None,
            },

            VSDINSIDE_VID => match pid {
                N4_PRO_PID => Some(Kind::N4Pro),
                _ => None,
            },

            MSDPRO_VID => match pid {
                MSDPRO_PID => Some(Kind::MsdPro),
                _ => None,
            },


            _ => None,
        }
    }

    /// There is no point relying on manufacturer/device names reported by the USB stack,
    /// so we return custom names for all the kinds of devices
    pub fn human_name(&self) -> String {
        match &self {
            Self::Akp05E => "Ajazz AKP05E",
            Self::N4EN => "Mirabox N4EN",
            Self::N4Pro => "VSDInside N4 Pro",
            Self::MsdPro => "Mars Gaming MSD-PRO (0B00:1003)",

        }
        .to_string()
    }

    /// Returns protocol version for device
    pub fn protocol_version(&self) -> usize {
        match self {
            Self::N4EN => 3,
            Self::Akp05E => 3,
            Self::N4Pro => 3,
            Self::MsdPro => 3,

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
}

#[derive(Debug, Clone)]
pub struct CandidateDevice {
    pub id: String,
    pub dev: HidDeviceInfo,
    pub kind: Kind,
}
