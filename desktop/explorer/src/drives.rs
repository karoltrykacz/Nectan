use crate::App;
use crate::Duration;
use crate::notifications::show_notification;
use futures_util::stream::StreamExt;
use slint::ComponentHandle;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_ulong};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use zbus::OwnedVariant;
use zbus::fdo::ObjectManagerProxy;
use zbus::names::{InterfaceName, MemberName};
use zbus::{Connection, OwnedObjectPath, Variant};
use zbus::{MatchRule, MessageStream, message::Type as MessageType};

type ManagedObjects = HashMap<
    OwnedObjectPath,
    HashMap<String, HashMap<String, OwnedVariant>>,
>;

#[derive(Debug, Clone)]
pub struct MountUsage {
    pub mount_point: PathBuf,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveType {
    UsbFlash,
    ExternalHdd,
    ExternalSsd,
    SdCard,
    Optical,
    Internal,
    Unknown,
}

impl DriveType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::UsbFlash => "USB Flash Drive",
            Self::ExternalHdd => "External HDD",
            Self::ExternalSsd => "External SSD",
            Self::SdCard => "SD Card",
            Self::Optical => "Optical Drive",
            Self::Internal => "Internal Drive",
            Self::Unknown => "Device",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PhysicalDrive {
    pub drive_type: DriveType,
    pub name: String,
    pub vendor: String,
    pub model: String,
    pub drive_path: OwnedObjectPath,
    pub block_device: Option<PathBuf>,
    pub total_size: u64,
    pub mounts: Vec<MountUsage>,
    pub ejectable: bool,
    pub can_power_off: bool,
}

pub async fn safely_remove_drive(
    drive: &PhysicalDrive,
) -> zbus::Result<()> {
    let conn = Connection::system().await?;
    let options: HashMap<&str, Variant> = HashMap::new();

    // 1. Find and unmount all mounted filesystems belonging to this drive
    let reply = conn
        .call_method(
            Some("org.freedesktop.UDisks2"),
            "/org/freedesktop/UDisks2",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;

    let objects: ManagedObjects = reply.body().deserialize()?;

    for (block_path, interfaces) in &objects {
        let Some(block_props) =
            interfaces.get("org.freedesktop.UDisks2.Block")
        else {
            continue;
        };
        let Some(fs_props) =
            interfaces.get("org.freedesktop.UDisks2.Filesystem")
        else {
            continue;
        };

        let parent_drive = block_props
            .get("Drive")
            .and_then(|v| OwnedObjectPath::try_from(v.clone()).ok());

        if parent_drive.as_ref() != Some(&drive.drive_path) {
            continue;
        }

        let mount_points: Vec<Vec<u8>> = fs_props
            .get("MountPoints")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        if !mount_points.is_empty() {
            conn.call_method(
                Some("org.freedesktop.UDisks2"),
                block_path.as_str(),
                Some("org.freedesktop.UDisks2.Filesystem"),
                "Unmount",
                &(&options,),
            )
            .await?;
        }
    }

    // 2. Eject removable media if supported (CD/DVD, SD card readers)
    if drive.ejectable {
        conn.call_method(
            Some("org.freedesktop.UDisks2"),
            drive.drive_path.as_str(),
            Some("org.freedesktop.UDisks2.Drive"),
            "Eject",
            &(&options,),
        )
        .await?;
    }

    // 3. Power off the drive port if supported (USB sticks, external SSDs/HDDs)
    if drive.can_power_off {
        conn.call_method(
            Some("org.freedesktop.UDisks2"),
            drive.drive_path.as_str(),
            Some("org.freedesktop.UDisks2.Drive"),
            "PowerOff",
            &(&options,),
        )
        .await?;
    }

    Ok(())
}
//
// pub async fn watch_drive_changes() -> zbus::Result<()> {
//     let conn = Connection::system().await?;
//
//     let obj_manager = ObjectManagerProxy::new(
//         &conn,
//         "org.freedesktop.UDisks2",
//         "/org/freedesktop/UDisks2",
//     )
//     .await?;
//
//     let mut added_stream =
//         obj_manager.receive_interfaces_added().await?;
//
//     let mut removed_stream =
//         obj_manager.receive_interfaces_removed().await?;
//
//     let prop_rule = MatchRule::builder()
//         .msg_type(MessageType::Signal)
//         .sender("org.freedesktop.UDisks2")
//         .interface(InterfaceName::from_static_str(
//             "org.freedesktop.DBus.Properties",
//         )?)
//         .member(MemberName::from_static_str("PropertiesChanged")?)
//         .build()?;
//
//     let mut prop_stream =
//         MessageStream::for_match_rule(prop_rule, &conn, None).await?;
//
//     let mut drives = list_drives().await?;
//
//     fn is_relevant(iface: &str) -> bool {
//         matches!(
//             iface,
//             "org.freedesktop.UDisks2.Drive"
//                 | "org.freedesktop.UDisks2.Filesystem"
//         )
//     }
//
//     loop {
//         let xd = added_stream.next().await.unwrap();
//         tokio::select! {
//             Some(signal) = added_stream.next() => {
//                 if let Ok(args) = signal.args() {
//                     if args
//                         .interfaces_and_properties()
//                         .keys()
//                         .any(|i| is_relevant(i.as_str()))
//                     {
//                         println!("New device {:#?}", args);
//                     }
//                 }
//             }
//             Some(signal) = removed_stream.next() => {
//                 if let Ok(args) = signal.args() {
//                     if args.interfaces().iter().any(|i| is_relevant(i.as_str())) {
//                         println!("Removed device {}", args.object_path());
//                     }
//                 }
//             }
//             Some(Ok(msg)) = prop_stream.next() => {
//                 let Ok((iface, changed, _invalidated)) = msg
//                     .body()
//                     .deserialize::<(String, HashMap<String, OwnedVariant>, Vec<String>)>()
//                 else {
//                     continue;
//                 };
//
//                 if iface == "org.freedesktop.UDisks2.Filesystem"
//                     && changed.contains_key("MountPoints")
//                 {
//                     println!("Mount state changed on {:?}", msg.header().path());
//                 }
//             }
//         }
//     }
// }
pub async fn watch_drive_changes(
    app: slint::Weak<App>,
) -> zbus::Result<()> {
    let conn = Connection::system().await?;

    let obj_manager = ObjectManagerProxy::new(
        &conn,
        "org.freedesktop.UDisks2",
        "/org/freedesktop/UDisks2",
    )
    .await?;

    let mut added_stream =
        obj_manager.receive_interfaces_added().await?;

    let mut removed_stream =
        obj_manager.receive_interfaces_removed().await?;

    let mut drives = list_drives().await?;

    loop {
        tokio::select! {
            Some(signal) = added_stream.next() => {
                let Ok(args) = signal.args() else {
                    continue;
                };

                let is_drive = args
                    .interfaces_and_properties()
                    .contains_key(
                        "org.freedesktop.UDisks2.Drive"
                    );

                if !is_drive {
                    continue;
                }

                let path = args.object_path().clone();

                tokio::time::sleep(
                    Duration::from_millis(300)
                ).await;

                let Ok(current_drives) = list_drives().await else {
                    continue;
                };

                for drive in &current_drives {
                    if drive.drive_path == path.clone().into()
                        && !drives.iter().any(|d|
                            d.drive_path == drive.drive_path
                        )
                    {
                        let title = drive.name.clone();
                        let subtitle = format!(
                            "{} connected",
                            drive.drive_type.as_str()
                        );

                        let _ = app.upgrade_in_event_loop(
                            move |w| {
                                show_notification(
                                    &w.as_weak(),
                                    title,
                                    subtitle,
                                );
                            }
                        );
                    }
                }

                drives = current_drives;
            }

            Some(signal) = removed_stream.next() => {
                let Ok(args) = signal.args() else {
                    continue;
                };

                if !args.interfaces().iter().any(|i|
                    i.as_str()
                        == "org.freedesktop.UDisks2.Drive"
                ) {
                    continue;
                }

                let path = args.object_path().clone();

                drives.retain(|d| d.drive_path != path.clone().into());
            }

            else => break,
        }
    }

    Ok(())
}

#[repr(C)]
struct StatVfs {
    f_bsize: c_ulong,
    f_frsize: c_ulong,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: c_ulong,
    #[cfg(target_pointer_width = "32")]
    __f_unused: c_int,
    f_flag: c_ulong,
    f_namemax: c_ulong,
    __f_spare: [c_int; 6],
}

unsafe extern "C" {
    fn statvfs(path: *const c_char, buf: *mut StatVfs) -> c_int;
}

fn get_mount_usage(mount_point: &Path) -> Option<MountUsage> {
    let c_path =
        CString::new(mount_point.as_os_str().as_bytes()).ok()?;
    let mut stat = MaybeUninit::<StatVfs>::uninit();

    let res = unsafe { statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if res != 0 {
        return None;
    }

    let stat = unsafe { stat.assume_init() };
    let block_size = stat.f_frsize as u64;

    let total_bytes = stat.f_blocks * block_size;
    let free_bytes = stat.f_bfree * block_size;
    let available_bytes = stat.f_bavail * block_size;
    let used_bytes = total_bytes.saturating_sub(free_bytes);

    Some(MountUsage {
        mount_point: mount_point.to_path_buf(),
        total_bytes,
        used_bytes,
        available_bytes,
    })
}

pub async fn list_drives() -> zbus::Result<Vec<PhysicalDrive>> {
    let conn = Connection::system().await?;

    let reply = conn
        .call_method(
            Some("org.freedesktop.UDisks2"),
            "/org/freedesktop/UDisks2",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;

    let objects: ManagedObjects = reply.body().deserialize()?;

    let mut drive_blocks: HashMap<
        OwnedObjectPath,
        (Option<PathBuf>, Vec<MountUsage>),
    > = HashMap::new();

    for interfaces in objects.values() {
        let Some(block_props) =
            interfaces.get("org.freedesktop.UDisks2.Block")
        else {
            continue;
        };

        let Some(drive_path) = block_props
            .get("Drive")
            .and_then(|v| OwnedObjectPath::try_from(v.clone()).ok())
        else {
            continue;
        };

        if drive_path.as_str() == "/" {
            continue;
        }

        let entry = drive_blocks
            .entry(drive_path)
            .or_insert_with(|| (None, Vec::new()));

        if !interfaces
            .contains_key("org.freedesktop.UDisks2.Partition")
        {
            if let Some(raw_dev) = block_props
                .get("Device")
                .and_then(|v| Vec::<u8>::try_from(v.clone()).ok())
            {
                if let Ok(c_str) = CStr::from_bytes_with_nul(&raw_dev)
                {
                    entry.0 = Some(PathBuf::from(
                        c_str.to_string_lossy().into_owned(),
                    ));
                }
            }
        }

        if let Some(fs_props) =
            interfaces.get("org.freedesktop.UDisks2.Filesystem")
        {
            let mount_points: Vec<Vec<u8>> = fs_props
                .get("MountPoints")
                .and_then(|v| v.clone().try_into().ok())
                .unwrap_or_default();

            if let Some(raw_mount) = mount_points.first() {
                if let Ok(c_str) =
                    CStr::from_bytes_with_nul(raw_mount)
                {
                    let path = PathBuf::from(
                        c_str.to_string_lossy().into_owned(),
                    );
                    if let Some(usage) = get_mount_usage(&path) {
                        entry.1.push(usage);
                    }
                }
            }
        }
    }

    let mut results = Vec::new();

    for (obj_path, interfaces) in &objects {
        let Some(drive_props) =
            interfaces.get("org.freedesktop.UDisks2.Drive")
        else {
            continue;
        };

        let vendor: String = drive_props
            .get("Vendor")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        let model: String = drive_props
            .get("Model")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        let name = match (vendor.trim(), model.trim()) {
            ("", "") => "Drive".to_string(),
            ("", m) => m.to_string(),
            (v, "") => v.to_string(),
            (v, m) => format!("{v} {m}"),
        };

        let ejectable = drive_props
            .get("Ejectable")
            .and_then(|v| bool::try_from(v.clone()).ok())
            .unwrap_or(false);

        let can_power_off = drive_props
            .get("CanPowerOff")
            .and_then(|v| bool::try_from(v.clone()).ok())
            .unwrap_or(false);

        let total_size = drive_props
            .get("Size")
            .and_then(|v| u64::try_from(v.clone()).ok())
            .unwrap_or(0);

        let (block_device, mounts) = drive_blocks
            .remove(obj_path)
            .unwrap_or((None, Vec::new()));

        let connection_bus: String = drive_props
            .get("ConnectionBus")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        let removable = drive_props
            .get("Removable")
            .and_then(|v| bool::try_from(v.clone()).ok())
            .unwrap_or(false);

        let optical = drive_props
            .get("Optical")
            .and_then(|v| bool::try_from(v.clone()).ok())
            .unwrap_or(false);

        let media: String = drive_props
            .get("Media")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();

        let rotation_rate: u16 = drive_props
            .get("RotationRate")
            .and_then(|v| u16::try_from(v.clone()).ok())
            .unwrap_or(0);

        let drive_type = if optical {
            DriveType::Optical
        } else if connection_bus == "usb" && removable {
            if rotation_rate > 0 {
                DriveType::ExternalHdd
            } else {
                DriveType::UsbFlash
            }
        } else if connection_bus == "usb" {
            if rotation_rate > 0 {
                DriveType::ExternalHdd
            } else {
                DriveType::ExternalSsd
            }
        } else if removable {
            DriveType::SdCard
        } else {
            DriveType::Internal
        };

        results.push(PhysicalDrive {
            drive_type,
            name,
            vendor,
            model,
            drive_path: obj_path.clone(),
            block_device,
            total_size,
            mounts,
            ejectable,
            can_power_off,
        });
    }

    Ok(results)
}
