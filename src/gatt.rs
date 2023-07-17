//! GATT [`Service`]s and [`Characteristic`]s exported by BLE devices.

use futures_util::StreamExt;
use zbus::{
    zvariant::{ObjectPath, Value},
    PropertyStream,
};

use crate::{uuid::Uuid, Error, Result, Session};

mod private {
    use zbus::{
        dbus_proxy,
        zvariant::{ObjectPath, SerializeDict, Type},
    };

    #[dbus_proxy(
        interface = "org.bluez.GattService1",
        default_service = "org.bluez",
        assume_defaults = false
    )]
    trait GattService {
        #[dbus_proxy(property, name = "UUID")]
        fn uuid(&self) -> zbus::Result<String>;

        #[dbus_proxy(property)]
        fn primary(&self) -> zbus::Result<bool>;
    }

    #[dbus_proxy(
        interface = "org.bluez.GattCharacteristic1",
        default_service = "org.bluez",
        assume_defaults = false
    )]
    trait GattCharacteristic {
        fn read_value(&self, options: &ReadOptions) -> zbus::Result<Vec<u8>>;
        fn write_value(&self, value: &[u8], options: &WriteOptions) -> zbus::Result<()>;

        fn start_notify(&self) -> zbus::Result<()>;
        fn stop_notify(&self) -> zbus::Result<()>;

        #[dbus_proxy(property, name = "UUID")]
        fn uuid(&self) -> zbus::Result<String>;

        #[dbus_proxy(property)]
        fn value(&self) -> zbus::Result<Vec<u8>>;

        #[dbus_proxy(property)]
        fn flags(&self) -> zbus::Result<Vec<String>>;

        #[dbus_proxy(property, name = "MTU")]
        fn mtu(&self) -> zbus::Result<u16>;
    }

    #[derive(SerializeDict, Type)]
    #[zvariant(signature = "dict")]
    pub struct ReadOptions {
        // FIXME: `pub` because zbus' `dbus_proxy` macro *always* generates public proxy types and
        // methods instead of copying the trait visibility
        offset: Option<u16>,
        mtu: Option<u16>,
        device: Option<ObjectPath<'static>>,
    }

    #[derive(Default, SerializeDict, Type)]
    #[zvariant(signature = "dict")]
    pub struct WriteOptions {
        // FIXME: `pub` because zbus' `dbus_proxy` macro *always* generates public proxy types and
        // methods instead of copying the trait visibility
        offset: Option<u16>,
        /// `command`, `request`, `reliable`
        #[zvariant(rename = "type")]
        ty: Option<&'static str>,
        mtu: Option<u16>,
        device: Option<ObjectPath<'static>>,
        link: Option<String>,
        #[zvariant(rename = "prepare-authorize")]
        prepare_authorize: Option<bool>,
    }
}

use self::private::{GattCharacteristicProxy, GattServiceProxy, WriteOptions};

/// A GATT service of a Bluetooth LE device.
///
/// To enumerate [`Service`]s, use [`Device::gatt_services`].
///
/// [`Device::gatt_services`]: crate::device::Device::gatt_services
pub struct Service {
    proxy: GattServiceProxy<'static>,
    session: Session,
}

impl Service {
    pub(crate) async fn new(session: Session, path: &ObjectPath<'static>) -> Result<Self> {
        Ok(Self {
            proxy: GattServiceProxy::new(&session.conn, path)
                .await
                .map_err(Error::from)?,
            session,
        })
    }

    /// Returns the [`Uuid`] identifying this [`Service`].
    pub async fn uuid(&self) -> Result<Uuid> {
        match self.proxy.uuid().await {
            Ok(uuid) => uuid.parse().map_err(Error::from),
            Err(e) => Err(Error::from(e)),
        }
    }

    /// Returns a [`bool`] indicating whether this [`Service`] is a primary service.
    ///
    /// If `false`, the service is secondary.
    pub async fn is_primary(&self) -> Result<bool> {
        self.proxy.primary().await.map_err(Error::from)
    }

    /// Returns the [`Characteristic`] associated with this [`Service`] identified by the given
    /// [`Uuid`].
    ///
    /// Returns an error if the [`Service`] does not expose any [`Characteristic`] with the given
    /// [`Uuid`].
    pub async fn characteristic(&self, uuid: Uuid) -> Result<Characteristic> {
        let objects = self
            .session
            .object_manager()
            .await?
            .get_managed_objects()
            .await
            .map_err(Error::from)?;

        let value = Value::from(uuid.to_string());
        for (path, intfs) in objects {
            if !path.starts_with(self.proxy.path().as_str()) {
                continue;
            }

            let Some(props) = intfs.get("org.bluez.GattCharacteristic1") else { continue };
            let Some(s) = props.get("UUID") else { continue };
            if **s == value {
                return Characteristic::new(&self.session, &path).await;
            }
        }

        Err(Error::from(format!(
            "no characteristic with UUID {} found in service",
            uuid
        )))
    }

    /// Returns a list of all [`Characteristic`]s associated with this [`Service`].
    pub async fn characteristics(&self) -> Result<Vec<Characteristic>> {
        let objects = self
            .session
            .object_manager()
            .await?
            .get_managed_objects()
            .await
            .map_err(Error::from)?;

        let mut characteristics = Vec::new();
        for (path, intfs) in objects {
            if path.starts_with(self.proxy.path().as_str())
                && intfs.contains_key("org.bluez.GattCharacteristic1")
            {
                characteristics.push(Characteristic::new(&self.session, &path).await?);
            }
        }

        Ok(characteristics)
    }
}

/// A Bluetooth characteristic that is part of some [`Service`].
///
/// A characteristic stores a value that can be (depending on the specific characteristic) read
/// and/or written by the host.
pub struct Characteristic {
    proxy: GattCharacteristicProxy<'static>,
}

impl Characteristic {
    async fn new(session: &Session, path: &ObjectPath<'static>) -> Result<Self> {
        Ok(Self {
            proxy: GattCharacteristicProxy::new(&session.conn, path)
                .await
                .map_err(Error::from)?,
        })
    }

    /// Returns the [`Uuid`] identifying this [`Characteristic`].
    ///
    /// The returned [`Uuid`] determines the data format of the characteristic's value. For standard
    /// services and characteristics, [`Uuid`]s are assigned by the Bluetooth SIG and documented in
    /// their "Assigned Numbers" document. For vendor-specific characteristics, consult the vendor
    /// for documentation.
    pub async fn uuid(&self) -> Result<Uuid> {
        match self.proxy.uuid().await {
            Ok(s) => s.parse().map_err(Error::from),
            Err(e) => Err(Error::from(e)),
        }
    }

    /// Returns the Maximum Transmission Unit (MTU) of this characteristic in Bytes.
    pub async fn mtu(&self) -> Result<u16> {
        self.proxy.mtu().await.map_err(Error::from)
    }

    /// Returns the [`CharacteristicFlags`] associated with this [`Characteristic`].
    ///
    /// These flags indicate which operations the [`Characteristic`] supports.
    pub async fn flags(&self) -> Result<CharacteristicFlags> {
        self.proxy
            .flags()
            .await
            .map_err(Error::from)
            .map(|flags| CharacteristicFlags { flags })
    }

    /// Enables notifications/indications for this [`Characteristic`] and returns a [`ValueStream`]
    /// that will report changes to the [`Characteristic`]'s value.
    pub async fn subscribe(&self) -> Result<ValueStream> {
        self.proxy.start_notify().await.map_err(Error::from)?;
        let stream = self.proxy.receive_value_changed().await;
        Ok(ValueStream { stream })
    }

    /// Writes a new value to this [`Characteristic`].
    pub async fn write(&self, value: &[u8]) -> Result<()> {
        self.proxy
            .write_value(value, &WriteOptions::default())
            .await
            .map_err(Error::from)
    }
}

/// A set of flags detailing the supported operations on a [`Characteristic`].
#[derive(Debug)]
pub struct CharacteristicFlags {
    flags: Vec<String>,
}

impl CharacteristicFlags {
    /// Returns a [`bool`] indicating whether the device can notify the host of changes made to the
    /// [`Characteristic`]'s value.
    ///
    /// If this returns `true`, [`Characteristic::subscribe`] can be used to obtain a
    /// [`ValueStream`] that reports every notification.
    pub fn can_notify(&self) -> bool {
        self.flags.iter().any(|s| s == "notify")
    }

    /// Returns a [`bool`] indicating whether the device supports sending *indications* of changes
    /// made to the [`Characteristic`]'s value.
    ///
    /// Indications work almost exactly like notifications, but include an acknowledgement by the
    /// GATT client (host).
    pub fn can_indicate(&self) -> bool {
        self.flags.iter().any(|s| s == "indicate")
    }

    /// Returns a [`bool`] indicating whether the device allows host-initiated reads of the
    /// [`Characteristic`]'s value.
    ///
    /// Note that many [`Characteristic`]s do *not* allow host-initiated reads, but *do* support
    /// device-initiated notifications (see [`CharacteristicFlags::can_notify`]).
    pub fn can_read(&self) -> bool {
        self.flags.iter().any(|s| s == "read")
    }

    /// Returns a [`bool`] indicating whether the device allows the host to set the
    /// [`Characteristic`]'s value.
    pub fn can_write(&self) -> bool {
        self.flags.iter().any(|s| s == "write")
    }
}

/// A stream of changes to the value of a [`Characteristic`].
///
/// Returned by [`Characteristic::subscribe`].
pub struct ValueStream {
    stream: PropertyStream<'static, Vec<u8>>,
}

impl ValueStream {
    /// Waits for the next notification or indication to arrive, and returns the new value of the
    /// [`Characteristic`].
    ///
    /// # Errors
    ///
    /// Once this method returns [`Err`], subsequent calls to it will generally not succeed. The
    /// caller should assume that something higher up has gone wrong that will not recover on its
    /// own. The [`ValueStream`] should be recreated.
    ///
    /// Note that this method is not guaranteed to fail if the [`Device`] is disconnected (it can
    /// block forever). It is recommended to use a timeout with this method (or on some containing
    /// future).
    ///
    /// [`Device`]: crate::device::Device
    pub async fn next(&mut self) -> Result<Vec<u8>> {
        match self.stream.next().await {
            Some(changed) => changed.get().await.map_err(Error::from),
            None => Err(Error::from("notification stream ended")),
        }
    }
}
