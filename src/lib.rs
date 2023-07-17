//! BlueZ D-Bus bindings.

mod adapter;
pub mod address;
pub mod device;
mod error;
pub mod gatt;
pub mod uuid;

pub use adapter::{Adapter, DeviceStream};
pub use error::{Error, Result};

use zbus::{fdo::ObjectManagerProxy, Connection};

/// A cloneable handle to a D-Bus connection.
///
/// This type is used to construct various objects in this library.
#[derive(Clone)]
pub struct Session {
    conn: Connection,
}

impl Session {
    /// Creates a new D-Bus connection.
    pub async fn new() -> Result<Self> {
        Ok(Self {
            conn: Connection::system().await.map_err(Error::from)?,
        })
    }

    /// Connects to the BlueZ D-Bus object manager.
    async fn object_manager(&self) -> Result<ObjectManagerProxy<'static>> {
        Ok(ObjectManagerProxy::builder(&self.conn)
            .destination("org.bluez")
            .map_err(Error::from)?
            .path("/")
            .map_err(Error::from)?
            .build()
            .await
            .map_err(Error::from)?)
    }
}
