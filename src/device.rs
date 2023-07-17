//! BlueZ [`Device`] access.

use core::fmt;
use std::str::FromStr;

use futures_util::StreamExt;
use zbus::{
    fdo::{PropertiesChangedStream, PropertiesProxy},
    zvariant::ObjectPath,
};

use crate::{
    address::{Address, AddressType},
    gatt::Service,
    uuid::Uuid,
    Error, Result, Session,
};

mod private {
    use zbus::dbus_proxy;

    #[dbus_proxy(
        interface = "org.bluez.Device1",
        default_service = "org.bluez",
        assume_defaults = false
    )]
    trait Device {
        async fn connect(&self) -> zbus::Result<()>;
        async fn disconnect(&self) -> zbus::Result<()>;

        #[dbus_proxy(property)]
        fn connected(&self) -> zbus::Result<bool>;

        #[dbus_proxy(property)]
        fn address(&self) -> zbus::Result<String>;

        #[dbus_proxy(property)]
        fn address_type(&self) -> zbus::Result<String>;

        #[dbus_proxy(property)]
        fn alias(&self) -> zbus::Result<String>;

        #[dbus_proxy(property)]
        fn rssi(&self) -> zbus::Result<i16>;

        #[dbus_proxy(property)]
        fn services_resolved(&self) -> zbus::Result<bool>;

        #[dbus_proxy(property, name = "UUIDs")]
        fn uuids(&self) -> zbus::Result<Vec<String>>;
    }
}

use private::DeviceProxy;

/// A reference to a remote BlueZ device.
///
/// Instances of this type can be obtained from [`Adapter::device_stream`][crate::Adapter::device_stream].
#[derive(Clone)]
pub struct Device {
    session: Session,
    proxy: DeviceProxy<'static>,
}

impl Device {
    pub(crate) async fn new(session: Session, path: ObjectPath<'static>) -> Result<Self> {
        let proxy = DeviceProxy::new(&session.conn, path)
            .await
            .map_err(Error::from)?;
        Ok(Self { session, proxy })
    }

    pub(crate) fn path(&self) -> ObjectPath<'static> {
        self.proxy.path().to_owned()
    }

    /// Returns the hardware [`Address`] of the device.
    pub async fn address(&self) -> Result<Address> {
        let string = self.proxy.address().await.map_err(Error::from)?;
        string.parse().map_err(Error::from)
    }

    /// Returns the type of the device's hardware [`Address`] returned by [`Device::address`].
    pub async fn address_type(&self) -> Result<AddressType> {
        let string = self.proxy.address_type().await.map_err(Error::from)?;
        AddressType::from_str(&string)
    }

    /// Returns the user-friendly name assigned to the device.
    pub async fn alias(&self) -> Result<String> {
        self.proxy.alias().await.map_err(Error::from)
    }

    /// Returns the Received Signal Strength Indicator (RSSI) of the remote device.
    pub async fn rssi(&self) -> Result<i16> {
        self.proxy.rssi().await.map_err(Error::from)
    }

    /// Returns the list of service [`Uuid`]s the device is advertising.
    ///
    /// This list is available without performing full service discovery or connecting to the
    /// device, but is typically truncated unless connected to or paired with the [`Device`].
    pub async fn service_uuids(&self) -> Result<Vec<Uuid>> {
        self.proxy
            .uuids()
            .await
            .map_err(Error::from)?
            .into_iter()
            .map(|s| Uuid::from_str(&*s).map_err(Error::from))
            .collect::<Result<Vec<_>>>()
    }

    /// Performs service discovery on a connected [`Device`] and returns all offered GATT services.
    ///
    /// # Errors
    ///
    /// This will return an error when attempted on a device that isn't connected. Call
    /// [`Device::connect`] before using this method.
    pub async fn gatt_services(&self) -> Result<Vec<Service>> {
        self.wait_services_resolved().await?;

        let mut services = Vec::new();
        let objects = self
            .session
            .object_manager()
            .await?
            .get_managed_objects()
            .await
            .map_err(Error::from)?;
        for (path, intf) in objects {
            if path.starts_with(self.proxy.path().as_str())
                && intf.contains_key("org.bluez.GattService1")
            {
                let res = Service::new(self.session.clone(), &path).await;
                match res {
                    Ok(service) => services.push(service),
                    Err(e) => log::error!("skipping GATT service at {} due to error: {}", path, e),
                }
            }
        }

        Ok(services)
    }

    async fn wait_services_resolved(&self) -> Result<()> {
        if !self.is_connected().await? {
            return Err(Error::from("device disconnected, cannot resolve services"));
        }

        let mut stream = self.proxy.receive_services_resolved_changed().await;
        if self.services_resolved().await? {
            log::debug!("services already resolved");
            return Ok(());
        }

        log::debug!("waiting for services to be resolved");
        while let Some(change) = stream.next().await {
            // TODO: this can hang indefinitely when the connection dies.
            // The DBus object disappears, so shouldn't the stream also end?
            if change.get().await.map_err(Error::from)? {
                log::debug!("service enumeration completed");
                return Ok(());
            }
        }

        // The stream ended. This may indicate that the device disappeared.
        Err(Error::from("failed to resolve services"))
    }

    async fn services_resolved(&self) -> Result<bool> {
        self.proxy.services_resolved().await.map_err(Error::from)
    }

    /// Establishes a connection to the device.
    ///
    /// Does nothing if the adapter is already connected to the device.
    pub async fn connect(&self) -> Result<()> {
        // Connecting to a device we're already connected to can result in a cryptic
        // `le-connection-abort-by-local` error, so ensure that this call succeeds if the device is
        // already connected.
        if self.is_connected().await? {
            return Ok(());
        }

        match self.proxy.connect().await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Connecting is racy, so check if we ended up connecting if it fails.
                if let Ok(true) = self.is_connected().await {
                    return Ok(());
                }
                return Err(Error::from(e));
            }
        }
    }

    /// Severs the connection to the device.
    ///
    /// Does nothing if the adapter is already disconnected from the device.
    pub async fn disconnect(&self) -> Result<()> {
        if !self.is_connected().await? {
            return Ok(());
        }

        match self.proxy.disconnect().await {
            Ok(()) => Ok(()),
            Err(e) => {
                if let Ok(false) = self.is_connected().await {
                    return Ok(());
                }
                return Err(Error::from(e));
            }
        }
    }

    /// Returns whether the adapter is currently connected to this device.
    pub async fn is_connected(&self) -> Result<bool> {
        self.proxy.connected().await.map_err(Error::from)
    }

    /// Returns a [`Changes`] stream that yields the [`PropertyName`] of properties when their
    /// values change.
    ///
    /// Only the [`PropertyName`]s passed as an argument will be subscribed to and yielded by the
    /// [`Changes`] stream.
    pub async fn property_change_stream<I: IntoIterator<Item = PropertyName>>(
        &self,
        properties: I,
    ) -> Result<Changes> {
        let interest = properties.into_iter().collect::<Vec<_>>();
        self.property_change_stream_impl(interest).await
    }

    async fn property_change_stream_impl(&self, interest: Vec<PropertyName>) -> Result<Changes> {
        // Property changes are signaled via the `PropertiesChanged` signal on the
        // `org.freedesktop.DBus.Properties` interface.
        let proxy = PropertiesProxy::builder(&self.session.conn)
            .path(self.proxy.path())
            .map_err(Error::from)?
            .destination("org.bluez")
            .map_err(Error::from)?
            .build()
            .await
            .map_err(Error::from)?;
        let stream = proxy
            .receive_properties_changed()
            .await
            .map_err(Error::from)?;
        Ok(Changes {
            stream,
            interest,
            change_buffer: Vec::new(),
        })
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Device")
            .field("path", self.proxy.path())
            .finish()
    }
}

/// A stream of [`Device`] property changes.
pub struct Changes {
    stream: PropertiesChangedStream<'static>,
    interest: Vec<PropertyName>,
    change_buffer: Vec<PropertyName>,
}

impl Changes {
    /// Asynchronously waits until a [`Device`] property changes, and returns the [`PropertyName`]
    /// whose value changed.
    ///
    /// Note that the order in which changed properties are yielded by this stream is unspecified.
    /// They can be yielded in any arbitrary order. Multiple changes to a property may be yielded
    /// multiple times, or may be collapsed into a single item.
    ///
    /// # Errors
    ///
    /// This method returns an error if the underlying notification stream ends, or if there is some
    /// other communication error. In general, the caller should assume that the stream is no longer
    /// operable if that happens.
    pub async fn wait(&mut self) -> Result<PropertyName> {
        if let Some(change) = self.change_buffer.pop() {
            return Ok(change);
        }

        loop {
            match self.stream.next().await {
                Some(changed) => {
                    let args = changed.args().map_err(Error::from)?;

                    log::trace!(
                        "{:?}: changed {:?}",
                        changed.path(),
                        args.changed_properties.keys(),
                    );

                    for prop in args.changed_properties.keys() {
                        if let Some(name) = PropertyName::from_str(prop) {
                            if self.interest.contains(&name) {
                                self.change_buffer.push(name);
                            }
                        }
                    }
                    for prop in &args.invalidated_properties {
                        if let Some(name) = PropertyName::from_str(prop) {
                            if self.interest.contains(&name) {
                                self.change_buffer.push(name);
                            }
                        }
                    }
                }
                None => return Err(Error::from("device change stream ended")),
            }

            if let Some(change) = self.change_buffer.pop() {
                return Ok(change);
            }
        }
    }
}

/// Identifies a [`Device`] property by name.
///
/// A property's value can be fetched via the methods on [`Device`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PropertyName {
    /// [`Device::alias`].
    Alias,
    /// [`Device::rssi`].
    Rssi,
    /// [`Device::service_uuids`].
    ServiceUuids,
    /// [`Device::is_connected`]. This allows detecting device disconnects.
    IsConnected,
}

impl PropertyName {
    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "Alias" => Self::Alias,
            "RSSI" => Self::Rssi,
            "UUIDs" => Self::ServiceUuids,
            "Connected" => Self::IsConnected,
            _ => return None,
        })
    }
}
