use std::{future::ready, pin::pin};

use futures_util::{
    stream::{select, FuturesUnordered},
    FutureExt, StreamExt,
};
use zbus::{
    dbus_proxy,
    fdo::{InterfacesAdded, InterfacesRemoved},
    zvariant::ObjectPath,
    SignalStream,
};

use crate::{
    address::{Address, AddressType},
    device::{Changes, Device, PropertyName},
    Error, Result, Session,
};

#[dbus_proxy(
    interface = "org.bluez.Adapter1",
    default_service = "org.bluez",
    assume_defaults = false
)]
trait Adapter {
    async fn start_discovery(&self) -> zbus::Result<()>;
    async fn stop_discovery(&self) -> zbus::Result<()>;

    #[dbus_proxy(property)]
    fn address(&self) -> zbus::Result<String>;

    #[dbus_proxy(property)]
    fn address_type(&self) -> zbus::Result<String>;

    #[dbus_proxy(property)]
    fn discovering(&self) -> zbus::Result<bool>;
}

/// A BlueZ Bluetooth adapter.
pub struct Adapter {
    session: Session,
    name: String,
    proxy: AdapterProxy<'static>,
}

impl Adapter {
    const PATH_PREFIX: &str = "/org/bluez/";

    /// Opens the system's default Bluetooth adapter.
    pub async fn open(session: &Session) -> Result<Self> {
        let mut adapters = Self::enumerate(session).await?.collect::<Vec<_>>();
        adapters.sort_by(|a, b| a.name.cmp(&b.name));

        if let Some(a) = adapters.into_iter().next() {
            return Ok(a);
        } else {
            return Err(Error::from("no adapter found"));
        }
    }

    /// Returns an iterator yielding all Bluetooth adapters on the system.
    pub async fn enumerate(session: &Session) -> Result<impl Iterator<Item = Self>> {
        log::debug!(
            "enumerating BlueZ adapters on connection {}",
            session.conn.server_guid()
        );

        let manager = session.object_manager().await?;
        let objects = manager.get_managed_objects().await.map_err(Error::from)?;
        let mut hci_names = Vec::new();
        for (obj_path, intfs) in &objects {
            if intfs.contains_key("org.bluez.Adapter1") {
                if let Some(name) = obj_path.strip_prefix(Self::PATH_PREFIX) {
                    log::debug!("found BlueZ adapter at path {}", obj_path);
                    hci_names.push(name.to_string());
                } else {
                    log::warn!("skipping adapter with unexpected path {}", obj_path);
                }
            }
        }

        let mut adapters = Vec::new();
        for name in hci_names {
            let path = format!("{}{}", Self::PATH_PREFIX, name);
            match AdapterProxy::new(&session.conn, path).await {
                Ok(proxy) => adapters.push(Adapter {
                    proxy,
                    name,
                    session: session.clone(),
                }),
                Err(e) => log::error!("failed to open adapter {}: {}", name, e),
            }
        }

        Ok(adapters.into_iter())
    }

    /// Returns the adapter's device name (eg. `hci0`).
    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Returns the Bluetooth device [`Address`] of this [`Adapter`].
    pub async fn address(&self) -> Result<Address> {
        let string = self.proxy.address().await.map_err(Error::from)?;
        string.parse().map_err(Error::from)
    }

    /// Returns the type of device [`Address`] used by this [`Adapter`].
    pub async fn address_type(&self) -> Result<AddressType> {
        let string = self.proxy.address_type().await.map_err(Error::from)?;
        AddressType::from_str(&string)
    }

    /// Starts the device discovery procedure.
    pub async fn start_discovery(&self) -> Result<()> {
        self.proxy.start_discovery().await.map_err(Error::from)
    }

    /// Stops the device discovery procedure.
    pub async fn stop_discovery(&self) -> Result<()> {
        self.proxy.stop_discovery().await.map_err(Error::from)
    }

    /// Returns whether this [`Adapter`] is currently performing device discovery.
    ///
    /// Device discovery can be started by calling [`Adapter::start_discovery`]. Note that the value
    /// of [`Adapter::is_discovering`] may not immediately change to reflect that discovery has been
    /// requested.
    pub async fn is_discovering(&self) -> Result<bool> {
        self.proxy.discovering().await.map_err(Error::from)
    }

    /// Returns a [`DeviceStream`] that will yield all [`Device`]s known to this [`Adapter`].
    ///
    /// This can be used to consume the result of device discovery. Note that paired and connected
    /// devices will also be yielded by the stream, even if those [`Device`]s aren't currently
    /// discoverable.
    pub async fn device_stream(&self) -> Result<DeviceStream> {
        self.device_set().await?.into_device_stream().await
    }

    /// Returns a [`DeviceSet`] containing all devices known to this [`Adapter`].
    ///
    /// If this [`Adapter`] is performing discovery, discovered devices will be added to the
    /// returned [`DeviceSet`] automatically. Otherwise, only "known" devices will be yielded by the
    /// [`DeviceSet`].
    async fn device_set(&self) -> Result<DeviceSet> {
        let manager = self.session.object_manager().await?;
        let signals = manager.receive_all_signals().await.map_err(Error::from)?;

        let mut devices = Vec::new();
        let mut changes = Vec::new();
        let objects = manager.get_managed_objects().await.map_err(Error::from)?;
        for (path, intfs) in objects {
            if path.starts_with(self.proxy.path().as_str())
                && intfs.contains_key("org.bluez.Device1")
            {
                let device = match Device::new(self.session.clone(), (*path).to_owned()).await {
                    Ok(dev) => dev,
                    Err(e) => {
                        log::warn!("skipping device at {}: {}", path, e);
                        continue;
                    }
                };

                let change = match device
                    .property_change_stream([PropertyName::Alias, PropertyName::ServiceUuids])
                    .await
                {
                    Ok(change) => change,
                    Err(e) => {
                        log::warn!(
                            "failed to listen to property changes for {}: {} (skipping device)",
                            path,
                            e
                        );
                        continue;
                    }
                };

                devices.push(device);
                changes.push(change);
            }
        }

        Ok(DeviceSet {
            session: self.session.clone(),
            adapter_path: self.proxy.path().to_owned(),
            added_removed_stream: signals,
            devices,
            change_streams: changes,
        })
    }
}

/// A set of [`Device`]s currently visible to an [`Adapter`].
///
/// Returned by [`Adapter::device_set`].
struct DeviceSet {
    session: Session,
    adapter_path: ObjectPath<'static>,
    added_removed_stream: SignalStream<'static>,
    change_streams: Vec<Changes>,
    devices: Vec<Device>,
}

// NB: `DeviceSet` is currently private because `DeviceStream` suffices for most things and I'm not
// confident that its API is any good.

impl DeviceSet {
    /// Returns a [`DeviceStream`] that yields both all currently known [`Device`]s, as well as all
    /// devices discovered in the future.
    ///
    /// Note that [`Device`]s can be yielded multiple times, for example when some of their
    /// properties change.
    async fn into_device_stream(self) -> Result<DeviceStream> {
        Ok(DeviceStream {
            to_yield: self.devices.clone(),
            set: self,
        })
    }

    async fn next_modification(&mut self) -> Option<Modification> {
        let added_removed_stream = self
            .added_removed_stream
            .by_ref()
            .filter_map(|message| async {
                if let Some(added) = InterfacesAdded::from_message(message.clone()) {
                    let args = added.args().ok()?;
                    if args.object_path.starts_with(self.adapter_path.as_str())
                        && args
                            .interfaces_and_properties
                            .contains_key("org.bluez.Device1")
                    {
                        let device = match Device::new(self.session.clone(), args.object_path.to_owned()).await {
                            Ok(dev) => dev,
                            Err(e) => {
                                log::warn!("skipping device at {}: {}", args.object_path, e);
                                return None;
                            }
                        };

                        let change = match device.property_change_stream([PropertyName::Alias, PropertyName::ServiceUuids]).await {
                            Ok(change) => change,
                            Err(e) => {
                                log::warn!(
                                    "failed to listen to property changes for {}: {} (skipping device)",
                                    args.object_path,
                                    e
                                );
                                return None;
                            }
                        };

                        Some(Modification::Add(device, change))
                    } else {
                        None
                    }
                } else if let Some(removed) = InterfacesRemoved::from_message(message) {
                    let args = removed.args().ok()?;
                    if args.object_path.starts_with(self.adapter_path.as_str())
                        && args.interfaces.contains(&"org.bluez.Device1")
                    {
                        if let Some(i) = self.devices.iter().position(|dev| dev.path() == args.object_path) {
                            Some(Modification::Remove(i))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

        let dev: FuturesUnordered<_> = self
            .change_streams
            .iter_mut()
            .enumerate()
            .map(|(i, change)| {
                change
                    .wait()
                    .map(move |prop| prop.map(|prop| Modification::Change(i, prop)))
            })
            .collect();
        let mut stream = pin!(select(
            added_removed_stream,
            dev.filter_map(|res| ready(res.ok()))
        ));

        stream.next().await
    }

    /// Asynchronously waits for and applies a change to this [`DeviceSet`].
    pub async fn change(&mut self) -> Result<DeviceSetChange<'_>> {
        match self.next_modification().await {
            Some(Modification::Add(device, change)) => {
                self.devices.push(device);
                self.change_streams.push(change);
                return Ok(DeviceSetChange::Added(&self.devices.last().unwrap()));
            }
            Some(Modification::Remove(i)) => {
                let device = self.devices.swap_remove(i);
                self.change_streams.swap_remove(i);
                return Ok(DeviceSetChange::Removed(device));
            }
            Some(Modification::Change(i, prop)) => {
                return Ok(DeviceSetChange::Changed(&self.devices[i], prop));
            }
            None => {
                return Err(Error::from("event stream ended (adapter disconnected?)"));
            }
        }
    }
}

enum Modification {
    Add(Device, Changes),
    Remove(usize),
    Change(usize, PropertyName),
}

/// Describes a change to a [`DeviceSet`], returned by [`DeviceSet::change`].
enum DeviceSetChange<'a> {
    /// The given [`Device`] was just added (discovered).
    Added(&'a Device),
    /// The given [`Device`] was removed (calling any methods on it will probably fail).
    Removed(Device),
    /// A property of the [`Device`] was changed (eg. the set of advertised services has been filled
    /// as part of device discovery, or the device's name was retrieved).
    ///
    /// Note that the [`DeviceSet`] only listens to changes to [`PropertyName::Alias`] and
    /// [`PropertyName::ServiceUuids`]. Any other property changes will not be reported.
    Changed(&'a Device, PropertyName),
}

/// A stream that yields newly discovered or changed [`Device`]s.
///
/// Returned by [`Adapter::device_stream`].
pub struct DeviceStream {
    to_yield: Vec<Device>,
    set: DeviceSet,
}

impl DeviceStream {
    /// Asynchronously yields the next [`Device`] seen by the [`Adapter`].
    ///
    /// Paired and connected [`Device`]s will be yielded by this stream, and if the [`Adapter`] is
    /// currently performing device discovery, the discovered [`Device`]s will also be yielded by
    /// this stream. Additionally, [`Device`]s can be yielded *multiple times* if their display name
    /// or set of advertised services changes.
    ///
    /// # Errors
    ///
    /// If this method returns an error, the caller should treat this as a permanent condition. It
    /// is likely that the [`Adapter`] has encountered a fatal error and needs to be reenumerated.
    ///
    /// Note that the returned future can take an arbitrary time to resolve (ie. there is no
    /// built-in timeout). The caller should implement its own timeout.
    pub async fn next(&mut self) -> Result<Device> {
        if let Some(device) = self.to_yield.pop() {
            return Ok(device);
        }

        loop {
            let change = self.set.change().await?;
            match change {
                DeviceSetChange::Added(dev) | DeviceSetChange::Changed(dev, _) => {
                    return Ok(dev.clone());
                }
                DeviceSetChange::Removed(_) => {}
            }
        }
    }
}
