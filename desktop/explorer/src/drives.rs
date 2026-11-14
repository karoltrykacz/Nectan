use crate::App;
use crate::Duration;
use crate::notifications::show_notification;
use crate::{DriveData, DrivesBridge, MountUsageData};
use futures_util::stream::StreamExt;
use slint::Weak;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_ulong};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use tracing::{error, info};
use zbus::OwnedVariant;
use zbus::fdo::ObjectManagerProxy;
use zbus::{Connection, OwnedObjectPath, Variant};
use zbus::{MatchRule, MessageStream};

type ManagedObjects = HashMap<
    OwnedObjectPath,
    HashMap<String, HashMap<String, OwnedVariant>>,
>;

const UDISKS: &str = "org.freedesktop.UDisks2";

#[derive(Debug, Clone)]
struct MountUsage {
    mount_point: PathBuf,
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DriveType {
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
struct PhysicalDrive {
    drive_type: DriveType,
    name: String,
    vendor: String,
    model: String,
    drive_path: OwnedObjectPath,
    block_device: Option<PathBuf>,
    total_size: u64,
    mounts: Vec<MountUsage>,
    ejectable: bool,
    can_power_off: bool,
}

pub fn setup_drives(w: &App) {
    let b = w.global::<DrivesBridge>();

    let weak = w.as_weak();
    b.on_refresh(move || {
        let weak = weak.clone();
        tokio::spawn(async move {
            match list_drives().await {
                Ok(drives) => push_drives(&weak, drives),
                Err(e) => error!("Failed to list drives: {e}"),
            }
        });
    });

    let weak = w.as_weak();
    b.on_eject(move |id| {
        info!("Eject callback fired for {id:?}");
        tokio::spawn(run_removal(
            weak.clone(),
            id.into(),
            Removal::Eject,
        ));
    });

    let weak = w.as_weak();
    b.on_power_off(move |id| {
        info!("Power-off callback fired for {id:?}");
        tokio::spawn(run_removal(
            weak.clone(),
            id.into(),
            Removal::PowerOff,
        ));
    });

    let weak = w.as_weak();
    tokio::spawn(async move {
        if let Err(e) = watch_drive_changes(weak).await {
            error!("Drive watcher stopped: {e}");
        }
    });
}

#[derive(Clone, Copy)]
enum Removal {
    Eject,
    PowerOff,
}

async fn run_removal(
    app: slint::Weak<App>,
    id: String,
    action: Removal,
) {
    let drive = match list_drives().await {
        Ok(drives) => {
            drives.into_iter().find(|d| d.drive_path.as_str() == id)
        }
        Err(e) => {
            notify(
                &app,
                "Drives".into(),
                format!("Could not read drives: {e}"),
            );
            return;
        }
    };

    let Some(drive) = drive else {
        eprintln!("[drives] no drive with id {id:?} found");
        notify(&app, "Drives".into(), "Drive not found".into());
        return;
    };

    eprintln!(
        "[drives] removing {:?} (type {:?}, ejectable {}, can_power_off {}, mounts {})",
        drive.name,
        drive.drive_type,
        drive.ejectable,
        drive.can_power_off,
        drive.mounts.len(),
    );

    let result = match action {
        Removal::Eject => eject_drive(&drive).await,
        Removal::PowerOff => power_off_drive(&drive).await,
    };

    match &result {
        Ok(()) => eprintln!("[drives] removal succeeded"),
        Err(e) => eprintln!("[drives] removal FAILED: {e:?}"),
    }

    match result {
        Ok(()) => notify(
            &app,
            drive.name.clone(),
            match action {
                Removal::Eject => "Ejected".to_string(),
                Removal::PowerOff => "Safe to unplug".to_string(),
            },
        ),
        Err(e) => notify(
            &app,
            drive.name.clone(),
            format!("Could not remove: {e}"),
        ),
    }

    if let Ok(drives) = list_drives().await {
        push_drives(&app, drives);
    }
}

fn notify(app: &slint::Weak<App>, title: String, subtitle: String) {
    let _ = app.upgrade_in_event_loop(move |w| {
        show_notification(&w.as_weak(), title, subtitle);
    });
}

fn to_drive_data(d: &PhysicalDrive) -> DriveData {
    let mounts: Vec<MountUsageData> = d
        .mounts
        .iter()
        .map(|m| MountUsageData {
            mount_point: m
                .mount_point
                .to_string_lossy()
                .as_ref()
                .into(),
            total: format_bytes(m.total_bytes).into(),
            used: format_bytes(m.used_bytes).into(),
            available: format_bytes(m.available_bytes).into(),
            ratio: if m.total_bytes > 0 {
                (m.used_bytes as f64 / m.total_bytes as f64) as f32
            } else {
                0.0
            },
            used_bytes: m.used_bytes as f32,
        })
        .collect();

    DriveData {
        id: d.drive_path.as_str().into(),
        kind: d.drive_type.as_str().into(),
        name: d.name.as_str().into(),
        vendor: d.vendor.as_str().into(),
        model: d.model.as_str().into(),
        block_device: d
            .block_device
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
        capacity: format_bytes(d.total_size).into(),
        capacity_bytes: d.total_size as f32,
        mounts: ModelRc::new(VecModel::from(mounts)),
        ejectable: d.ejectable,
        can_power_off: d.can_power_off,
        internal: d.drive_type == DriveType::Internal,
    }
}

fn push_drives(app: &slint::Weak<App>, drives: Vec<PhysicalDrive>) {
    let _ = app.upgrade_in_event_loop(move |w| {
        let bridge = w.global::<DrivesBridge>();

        let prev_id = bridge
            .get_drives()
            .row_data(bridge.get_selected().max(0) as usize)
            .map(|d| d.id);

        let data: Vec<DriveData> =
            drives.iter().map(to_drive_data).collect();

        let internal = data.iter().filter(|d| d.internal).count();
        let external = data.len() - internal;

        let selected = prev_id
            .and_then(|id| data.iter().position(|d| d.id == id))
            .unwrap_or(0);

        bridge.set_internal_count(internal as i32);
        bridge.set_external_count(external as i32);
        bridge.set_drives(ModelRc::new(VecModel::from(data)));
        bridge.set_selected(selected as i32);
    });
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    if bytes == 0 {
        return "0 B".to_string();
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn ensure_removable(drive: &PhysicalDrive) -> zbus::Result<()> {
    if drive.drive_type == DriveType::Internal {
        return Err(zbus::Error::Failure(
            "refusing to unmount an internal drive".into(),
        ));
    }
    Ok(())
}

async fn managed_objects(
    conn: &Connection,
) -> zbus::Result<ManagedObjects> {
    let reply = conn
        .call_method(
            Some(UDISKS),
            "/org/freedesktop/UDisks2",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;

    reply.body().deserialize()
}

async fn unmount_all(
    conn: &Connection,
    drive: &PhysicalDrive,
) -> zbus::Result<()> {
    let options: HashMap<&str, Variant> = HashMap::new();
    let objects = managed_objects(conn).await?;

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
            eprintln!("[drives] unmounting {}", block_path.as_str());
            conn.call_method(
                Some(UDISKS),
                block_path.as_str(),
                Some("org.freedesktop.UDisks2.Filesystem"),
                "Unmount",
                &(&options,),
            )
            .await?;
        }
    }

    Ok(())
}

async fn drive_call(
    conn: &Connection,
    drive: &PhysicalDrive,
    method: &str,
) -> zbus::Result<()> {
    let options: HashMap<&str, Variant> = HashMap::new();

    eprintln!(
        "[drives] calling Drive.{method} on {}",
        drive.drive_path.as_str()
    );
    conn.call_method(
        Some(UDISKS),
        drive.drive_path.as_str(),
        Some("org.freedesktop.UDisks2.Drive"),
        method,
        &(&options,),
    )
    .await?;

    Ok(())
}

async fn eject_drive(drive: &PhysicalDrive) -> zbus::Result<()> {
    ensure_removable(drive)?;
    if !drive.ejectable {
        return Err(zbus::Error::Failure(
            "this drive cannot be ejected".into(),
        ));
    }

    let conn = Connection::system().await?;
    unmount_all(&conn, drive).await?;
    drive_call(&conn, drive, "Eject").await
}

async fn power_off_drive(drive: &PhysicalDrive) -> zbus::Result<()> {
    ensure_removable(drive)?;
    if !drive.can_power_off {
        return Err(zbus::Error::Failure(
            "this drive cannot be powered off".into(),
        ));
    }

    let conn = Connection::system().await?;
    unmount_all(&conn, drive).await?;
    drive_call(&conn, drive, "PowerOff").await
}

async fn safely_remove_drive(
    drive: &PhysicalDrive,
) -> zbus::Result<()> {
    ensure_removable(drive)?;

    let conn = Connection::system().await?;
    unmount_all(&conn, drive).await?;

    if drive.can_power_off {
        drive_call(&conn, drive, "PowerOff").await?;
    } else if drive.ejectable {
        drive_call(&conn, drive, "Eject").await?;
    }

    Ok(())
}

fn is_relevant(iface: &str) -> bool {
    matches!(
        iface,
        "org.freedesktop.UDisks2.Drive"
            | "org.freedesktop.UDisks2.Block"
            | "org.freedesktop.UDisks2.Filesystem"
    )
}

async fn reload(
    app: &slint::Weak<App>,
    conn: &Connection,
    known: &mut HashSet<String>,
    announce: bool,
) -> zbus::Result<()> {
    let drives = list_drives_on(conn).await?;

    if announce {
        for d in drives
            .iter()
            .filter(|d| !known.contains(d.drive_path.as_str()))
        {
            notify(
                app,
                d.name.clone(),
                format!("{} connected", d.drive_type.as_str()),
            );
        }
    }

    *known =
        drives.iter().map(|d| d.drive_path.to_string()).collect();

    push_drives(app, drives);
    Ok(())
}

async fn watch_drive_changes(
    app: slint::Weak<App>,
) -> zbus::Result<()> {
    let conn = Connection::system().await?;

    let obj_manager = ObjectManagerProxy::new(
        &conn,
        UDISKS,
        "/org/freedesktop/UDisks2",
    )
    .await?;

    let mut added_stream =
        obj_manager.receive_interfaces_added().await?;
    let mut removed_stream =
        obj_manager.receive_interfaces_removed().await?;

    let prop_rule = MatchRule::try_from(
        "type='signal',\
         sender='org.freedesktop.UDisks2',\
         interface='org.freedesktop.DBus.Properties',\
         member='PropertiesChanged'",
    )?;

    let mut prop_stream =
        MessageStream::for_match_rule(prop_rule, &conn, None).await?;

    let mut known = HashSet::new();
    reload(&app, &conn, &mut known, false).await?;

    let mut pending = false;

    loop {
        tokio::select! {
            Some(signal) = added_stream.next() => {
                if let Ok(args) = signal.args() {
                    if args
                        .interfaces_and_properties()
                        .keys()
                        .any(|i| is_relevant(i.as_str()))
                    {
                        pending = true;
                    }
                }
            }

            Some(signal) = removed_stream.next() => {
                if let Ok(args) = signal.args() {
                    if args.interfaces().iter().any(|i| is_relevant(i.as_str())) {
                        pending = true;
                    }
                }
            }

            Some(Ok(msg)) = prop_stream.next() => {
                let Ok((iface, changed, invalidated)) = msg
                    .body()
                    .deserialize::<(String, HashMap<String, OwnedVariant>, Vec<String>)>()
                else {
                    continue;
                };

                let mounts_changed = iface == "org.freedesktop.UDisks2.Filesystem"
                    && (changed.contains_key("MountPoints")
                        || invalidated.iter().any(|k| k == "MountPoints"));

                if mounts_changed || iface == "org.freedesktop.UDisks2.Drive" {
                    pending = true;
                }
            }

            _ = tokio::time::sleep(Duration::from_millis(300)), if pending => {
                pending = false;
                if let Err(e) = reload(&app, &conn, &mut known, true).await {
                    eprintln!("failed to reload drives: {e}");
                }
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

async fn list_drives() -> zbus::Result<Vec<PhysicalDrive>> {
    let conn = Connection::system().await?;
    list_drives_on(&conn).await
}

async fn list_drives_on(
    conn: &Connection,
) -> zbus::Result<Vec<PhysicalDrive>> {
    let objects = managed_objects(conn).await?;

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
        let vendor = vendor.trim().to_string();

        let model: String = drive_props
            .get("Model")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();
        let model = model.trim().to_string();

        let name = match (vendor.as_str(), model.as_str()) {
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

        let (block_device, mut mounts) = drive_blocks
            .remove(obj_path)
            .unwrap_or((None, Vec::new()));

        mounts.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));

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

    results.sort_by(|a, b| {
        (a.drive_type != DriveType::Internal)
            .cmp(&(b.drive_type != DriveType::Internal))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(results)
}
