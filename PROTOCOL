Nectan uses simple, binary protocol to exchange files with other devices.

1. Terms and definitions

Commonly used terms in this document are described below.

Identity: elliptic curve key pair generated at the first launch of the application.
Device id: public half of the identity.
Signing key: private half of the identity.
Endpoint id: globally unique identifier created as a cryptographic key.
#### 2 . Local discovery
At the start of the application Nectan launches MDNS-like endpoint discovery service. 
This allows Nectan to find and connect to devices that are on your local network.  See the [swarm-discovery](https://docs.rs/swarm-discovery/latest/swarm_discovery/)crate for more details. This mode of discovery doesn't require any external serjjvers.
#### 3. Global discovery 
To reach devices outside local network device announces itself and provides list of devices that it wishes to connect to.

(Presented payloads match exactly with the code)
```
struct EndpointAnnouncePayload {
    device_id: DeviceId,
    endpoint_id: EndpointId,
    signature: Signature,
	devices: Vec<DeviceId>
}
```

After successful announcement server responds with following response:
```
struct EndpointAnncounceResponse {
	seq_num: u64,
	devices: Vec<(DeviceId, EndpointId)>,
}
```

Subsequently, client will attempt to reach each one of the devices using iroh networking stack. For more details visit [iroh](https://docs.iroh.computer/what-is-iroh).

#### 4. Pairing with one-time-code
This is convenient way to connect two devices heavily inspired by polish banking system [BLIK](https://en.wikipedia.org/wiki/Blik).  Codes are uniquely identified by 6-digit [one-time codes](https://en.wikipedia.org/wiki/Time-based_One-Time_Password "Time-based One-Time Password"), each time valid for up to 1,5 minutes, which the user obtains using their app. The code can then be typed on remote device. The finalization of connection requires final acceptation from device that accepts the connection. 
##### 4.1 Code issuing
```
struct CodeIssueRequest {
    device_id: DeviceId,
    code: String,
    seq_num: u64,
    signature: Signature,
}
```
##### 4.1 Code lookup
```
struct CodeLookupRequest {
    code: String,
}

/// Sucessful response (code session was found and not expired)
struct CodeLookupResponse {
    device_id: EndpointId,
}
```

If code lookup is successful, device connects to the resolved endpoint.
#### 5. Hello
At the begining of connection both devices authenticate their identity using signatures.
```
struct Hello {
    id: DeviceId,
    username: Username,
    signature: Signature,
};
```
If authentication fails connection will be immediately aborted. Otherwise there are two cases:
- device already known - just continue with typical connection handling.
- device is not known - user will be prompted to accept the connection
#### 6. Offers
Connected devices will send each other transfers offer which are made of list of items they wish to transfer, number of items and total size.
```
struct TransferOffer {
	/// Typically name of fictional creature, but may be modified
	transfer_name: String,
    transfer_id: Uuid,
    pub entries_num: u32,
    pub total_size: u64,
    pub entries: Vec<TransferEntry>,
}
struct TransferEntry {
    path: PathBuf, 
    size: u64,
    is_dir: bool,
}
```
After offer is accepted, device will accept incoming bytes and store them in temporary files. After file is successfully received file will be renamed to original name.

#### 7. Security and redundancy
All transfers are end to end encrypted (even those in local network boundary). Nectan doesn't modify, inspect, strip or loosely compress sent contents. 

#### Summary
In summary Nectan has only 5 endpoints.
route("/ping")
route("/announce_endpoint")
route("/nonce")
route("/issue_code")
route("/find_code_session")

There is no difference in the protocol between the different device types or operating systems.