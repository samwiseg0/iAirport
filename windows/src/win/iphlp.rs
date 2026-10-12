//! IP Helper access: adapter addresses, default-route owner and byte counters.

use std::ffi::CStr;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GetBestInterfaceEx, GetIfEntry2, GAA_FLAG_INCLUDE_GATEWAYS,
    GAA_FLAG_INCLUDE_PREFIX, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
    GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH, MIB_IF_ROW2,
};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
};

use crate::ip_state::{IpState, Ipv6AddressInfo, Ipv6Origin};
use crate::throughput::CounterSample;

#[derive(Clone)]
pub struct AdapterInfo {
    pub index: u32,
    pub ipv6_index: u32,
    pub luid: NET_LUID_LH,
    pub ip: IpState,
}

enum Address {
    V4(std::net::Ipv4Addr),
    V6(std::net::Ipv6Addr),
}

fn decode(address: &SOCKET_ADDRESS) -> Option<Address> {
    if address.lpSockaddr.is_null() {
        return None;
    }
    // SAFETY: iSockaddrLength bounds each read; the family is checked first.
    unsafe {
        let family = (*address.lpSockaddr).sa_family;
        if family == AF_INET
            && address.iSockaddrLength as usize >= std::mem::size_of::<SOCKADDR_IN>()
        {
            let v4 = (address.lpSockaddr as *const SOCKADDR_IN).read_unaligned();
            let raw = v4.sin_addr.S_un.S_addr;
            Some(Address::V4(std::net::Ipv4Addr::from(raw.to_ne_bytes())))
        } else if family == AF_INET6
            && address.iSockaddrLength as usize >= std::mem::size_of::<SOCKADDR_IN6>()
        {
            let v6 = (address.lpSockaddr as *const SOCKADDR_IN6).read_unaligned();
            Some(Address::V6(std::net::Ipv6Addr::from(v6.sin6_addr.u.Byte)))
        } else {
            None
        }
    }
}

/// Finds the adapter whose `AdapterName` equals the WLAN interface GUID
/// string and reads its address state.
pub fn adapter(guid_string: &str) -> Option<AdapterInfo> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_INCLUDE_PREFIX
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u8> = Vec::new();
    for _ in 0..4 {
        buffer.resize(size as usize, 0);
        // SAFETY: the buffer holds `size` bytes; the API writes within it or
        // reports the size it needs.
        let code = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                flags,
                ptr::null(),
                buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        match code {
            ERROR_SUCCESS => break,
            ERROR_BUFFER_OVERFLOW => continue,
            _ => return None,
        }
    }
    let wanted = guid_string.to_ascii_uppercase();
    let mut node = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: the list is a chain of structs inside `buffer`, terminated by
    // a null Next pointer; sub-lists are read the same way.
    unsafe {
        while !node.is_null() {
            let item = &*node;
            let name = if item.AdapterName.is_null() {
                String::new()
            } else {
                CStr::from_ptr(item.AdapterName as *const _)
                    .to_string_lossy()
                    .to_ascii_uppercase()
            };
            if name == wanted {
                let mut ip = IpState::default();
                let mut unicast = item.FirstUnicastAddress;
                while !unicast.is_null() {
                    let entry = &*unicast;
                    match decode(&entry.Address) {
                        Some(Address::V4(v4)) => {
                            ip.ipv4.push(v4.to_string());
                            ip.ipv4_prefixes.push(Some(entry.OnLinkPrefixLength as u32));
                        }
                        Some(Address::V6(v6)) => {
                            let origin = Ipv6Origin {
                                prefix_origin: entry.PrefixOrigin as u32,
                                suffix_origin: entry.SuffixOrigin as u32,
                                dad_state: entry.DadState as u32,
                                preferred_lifetime: entry.PreferredLifetime,
                            };
                            ip.ipv6.push(Ipv6AddressInfo::new(
                                &v6.to_string(),
                                Some(entry.OnLinkPrefixLength as u32),
                                origin,
                            ));
                        }
                        None => {}
                    }
                    unicast = entry.Next;
                }
                let mut gateway = item.FirstGatewayAddress;
                while !gateway.is_null() {
                    let entry = &*gateway;
                    match decode(&entry.Address) {
                        Some(Address::V4(v4)) if ip.ipv4_router.is_none() => {
                            ip.ipv4_router = Some(v4.to_string())
                        }
                        Some(Address::V6(v6)) if ip.ipv6_router.is_none() => {
                            ip.ipv6_router = Some(v6.to_string())
                        }
                        _ => {}
                    }
                    gateway = entry.Next;
                }
                let index = item.Anonymous1.Anonymous.IfIndex;
                let ipv6_index = item.Ipv6IfIndex;
                ip.primary_v4 = ip.has_ipv4() && best_interface_v4() == Some(index);
                ip.primary_v6 = ip.has_global_ipv6() && best_interface_v6() == Some(ipv6_index);
                return Some(AdapterInfo {
                    index,
                    ipv6_index,
                    luid: item.Luid,
                    ip,
                });
            }
            node = item.Next;
        }
    }
    None
}

/// The interface Windows would route a public IPv4 destination through.
fn best_interface_v4() -> Option<u32> {
    let mut target = SOCKADDR_IN {
        sin_family: AF_INET,
        ..Default::default()
    };
    target.sin_addr.S_un.S_addr = u32::from_ne_bytes([8, 8, 8, 8]);
    let mut index = 0u32;
    // SAFETY: target is a fully initialised SOCKADDR_IN.
    let code =
        unsafe { GetBestInterfaceEx(&target as *const SOCKADDR_IN as *const SOCKADDR, &mut index) };
    (code == NO_ERROR).then_some(index)
}

fn best_interface_v6() -> Option<u32> {
    let mut target = SOCKADDR_IN6 {
        sin6_family: AF_INET6,
        ..Default::default()
    };
    target.sin6_addr.u.Byte =
        std::net::Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888).octets();
    let mut index = 0u32;
    // SAFETY: target is a fully initialised SOCKADDR_IN6.
    let code = unsafe {
        GetBestInterfaceEx(
            &target as *const SOCKADDR_IN6 as *const SOCKADDR,
            &mut index,
        )
    };
    (code == NO_ERROR).then_some(index)
}

pub fn counters(luid: NET_LUID_LH) -> Option<CounterSample> {
    let mut row = MIB_IF_ROW2 {
        InterfaceLuid: luid,
        ..Default::default()
    };
    // SAFETY: row is a zeroed MIB_IF_ROW2 with the LUID set as the key.
    let code = unsafe { GetIfEntry2(&mut row) };
    if code != NO_ERROR {
        return None;
    }
    Some(CounterSample {
        bytes_in: row.InOctets,
        bytes_out: row.OutOctets,
    })
}
