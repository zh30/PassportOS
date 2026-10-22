//! DHCP + SNTP on the station interface, inside `wifi_worker`.
//! The UI task never awaits the radio. UTC, no timezone.

use embassy_time::{Duration, Instant, Timer};
use esp_println::println;
use esp_radio::wifi::Interface;

const NTP_DELTA: u32 = 2_208_988_800;
const NTP_IPS: [[u8; 4]; 2] = [[162, 159, 200, 1], [216, 239, 35, 0]];

/// UTC unix seconds, or `None` if DHCP / the server does not answer.
pub async fn utc_unix() -> Option<u32> {
    let Some(mut iface) = Interface::try_station() else {
        println!("[clock] sntp no station");
        return None;
    };
    let mac = iface.mac_address();
    println!(
        "[clock] sntp mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    drain(&mut iface);
    let xid = u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]) | 0x0100_0000;
    let mut frame = [0u8; 576];

    let mut offer = None;
    for _attempt in 0..3 {
        let n = dhcp_frame(&mut frame, &mac, xid, 1, &[0; 4], &[0; 4]);
        if !send(&mut iface, &frame[..n]).await {
            println!("[clock] sntp dhcp tx fail");
            return None;
        }
        if let Some(got) = wait_dhcp(&mut iface, xid, 2, Duration::from_secs(2)).await {
            offer = Some(got);
            break;
        }
    }
    let Some(offer) = offer else {
        println!("[clock] sntp no offer");
        return None;
    };
    println!(
        "[clock] sntp offer {}.{}.{}.{}",
        offer.yiaddr[0], offer.yiaddr[1], offer.yiaddr[2], offer.yiaddr[3]
    );

    let mut ack = None;
    for _ in 0..3 {
        let n = dhcp_frame(&mut frame, &mac, xid, 3, &offer.yiaddr, &offer.server);
        if !send(&mut iface, &frame[..n]).await {
            println!("[clock] sntp dhcp req fail");
            return None;
        }
        if let Some(got) = wait_dhcp(&mut iface, xid, 5, Duration::from_secs(2)).await {
            ack = Some(got);
            break;
        }
    }
    let Some(mut ack) = ack else {
        println!("[clock] sntp no ack");
        return None;
    };
    if ack.router == [0, 0, 0, 0] {
        ack.router = offer.router;
    }
    let ip = ack.yiaddr;
    let gw = ack.router;
    println!(
        "[clock] sntp ip {}.{}.{}.{} gw {}.{}.{}.{}",
        ip[0], ip[1], ip[2], ip[3], gw[0], gw[1], gw[2], gw[3]
    );

    let mut gw_mac = None;
    for _ in 0..3 {
        if let Some(mac) = arp(&mut iface, &mac, &ip, &gw).await {
            gw_mac = Some(mac);
            break;
        }
    }
    let Some(gw_mac) = gw_mac else {
        println!("[clock] sntp no arp");
        return None;
    };

    for dest in NTP_IPS {
        let n = ntp_frame(&mut frame, &mac, &gw_mac, &ip, &dest);
        if !send(&mut iface, &frame[..n]).await {
            continue;
        }
        if let Some(unix) = wait_ntp(&mut iface, Duration::from_secs(3)).await {
            return Some(unix);
        }
        println!(
            "[clock] sntp timeout {}.{}.{}.{}",
            dest[0], dest[1], dest[2], dest[3]
        );
    }
    None
}

struct Lease {
    yiaddr: [u8; 4],
    server: [u8; 4],
    router: [u8; 4],
}

fn drain(iface: &mut Interface) {
    while let Some((rx, _tx)) = iface.receive() {
        rx.consume_token(|_| ());
    }
}

async fn send(iface: &mut Interface, frame: &[u8]) -> bool {
    for _ in 0..20 {
        if let Some(tx) = iface.transmit() {
            tx.consume_token(frame.len(), |buf| {
                buf[..frame.len()].copy_from_slice(frame);
            });
            return true;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
    false
}

async fn wait_dhcp(
    iface: &mut Interface,
    xid: u32,
    msg: u8,
    budget: Duration,
) -> Option<Lease> {
    let deadline = Instant::now() + budget;
    let mut buf = [0u8; 576];
    let mut seen = 0u8;
    while Instant::now() < deadline {
        if let Some((rx, _tx)) = iface.receive() {
            let n = rx.consume_token(|data| {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                n
            });
            seen = seen.saturating_add(1);
            if let Some(lease) = parse_dhcp(&buf[..n], xid, msg) {
                return Some(lease);
            }
        } else {
            Timer::after(Duration::from_millis(20)).await;
        }
    }
    if seen == 0 {
        println!("[clock] rx none");
    }
    None
}

async fn arp(iface: &mut Interface, mac: &[u8; 6], ip: &[u8; 4], gw: &[u8; 4]) -> Option<[u8; 6]> {
    let mut frame = [0u8; 42];
    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(mac);
    frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
    frame[14..16].copy_from_slice(&1u16.to_be_bytes());
    frame[16..18].copy_from_slice(&0x0800u16.to_be_bytes());
    frame[18] = 6;
    frame[19] = 4;
    frame[20..22].copy_from_slice(&1u16.to_be_bytes());
    frame[22..28].copy_from_slice(mac);
    frame[28..32].copy_from_slice(ip);
    frame[38..42].copy_from_slice(gw);
    if !send(iface, &frame).await {
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut buf = [0u8; 64];
    let mut logged = false;
    while Instant::now() < deadline {
        if let Some((rx, _tx)) = iface.receive() {
            let n = rx.consume_token(|data| {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                n
            });
            if n >= 42 && buf[12] == 0x08 && buf[13] == 0x06 && buf[21] == 2 {
                if buf[28..32] == *gw {
                    let mut mac = [0u8; 6];
                    mac.copy_from_slice(&buf[22..28]);
                    return Some(mac);
                }
                if !logged {
                    logged = true;
                    println!(
                        "[clock] arp reply {}.{}.{}.{}",
                        buf[28], buf[29], buf[30], buf[31]
                    );
                }
            }
        } else {
            Timer::after(Duration::from_millis(20)).await;
        }
    }
    None
}

async fn wait_ntp(iface: &mut Interface, budget: Duration) -> Option<u32> {
    let deadline = Instant::now() + budget;
    let mut buf = [0u8; 576];
    while Instant::now() < deadline {
        if let Some((rx, _tx)) = iface.receive() {
            let n = rx.consume_token(|data| {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                n
            });
            if let Some(unix) = parse_ntp(&buf[..n]) {
                return Some(unix);
            }
        } else {
            Timer::after(Duration::from_millis(20)).await;
        }
    }
    None
}

fn dhcp_frame(
    frame: &mut [u8],
    mac: &[u8; 6],
    xid: u32,
    msg: u8,
    req_ip: &[u8; 4],
    server: &[u8; 4],
) -> usize {
    frame.fill(0);
    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let bootp = 240 + if msg == 1 { 8 } else { 16 };
    let udp_len = 8 + bootp;
    let ip_len = 20 + udp_len;
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&(ip_len as u16).to_be_bytes());
    frame[22] = 64;
    frame[23] = 17;
    frame[26..30].copy_from_slice(&[0, 0, 0, 0]);
    frame[30..34].copy_from_slice(&[255, 255, 255, 255]);
    let sum = ip_checksum(&frame[14..34]);
    frame[24..26].copy_from_slice(&sum.to_be_bytes());
    frame[34..36].copy_from_slice(&68u16.to_be_bytes());
    frame[36..38].copy_from_slice(&67u16.to_be_bytes());
    frame[38..40].copy_from_slice(&(udp_len as u16).to_be_bytes());
    let b = 42;
    frame[b] = 1;
    frame[b + 1] = 1;
    frame[b + 2] = 6;
    frame[b + 4..b + 8].copy_from_slice(&xid.to_be_bytes());
    frame[b + 10..b + 12].copy_from_slice(&0x8000u16.to_be_bytes());
    frame[b + 28..b + 34].copy_from_slice(mac);
    frame[b + 236..b + 240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    let mut o = b + 240;
    frame[o] = 53;
    frame[o + 1] = 1;
    frame[o + 2] = msg;
    o += 3;
    if msg == 3 {
        frame[o] = 50;
        frame[o + 1] = 4;
        frame[o + 2..o + 6].copy_from_slice(req_ip);
        o += 6;
        frame[o] = 54;
        frame[o + 1] = 4;
        frame[o + 2..o + 6].copy_from_slice(server);
        o += 6;
    } else {
        frame[o] = 55;
        frame[o + 1] = 2;
        frame[o + 2] = 1;
        frame[o + 3] = 3;
        o += 4;
    }
    frame[o] = 255;
    o + 1
}

fn parse_dhcp(frame: &[u8], xid: u32, msg: u8) -> Option<Lease> {
    if frame.len() < 42 + 240 || frame[12] != 0x08 || frame[13] != 0x00 || frame[23] != 17 {
        return None;
    }
    let b = 42;
    if frame[b] != 2 || frame[b + 4..b + 8] != xid.to_be_bytes() {
        return None;
    }
    if frame[b + 236..b + 240] != [0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let mut yiaddr = [0u8; 4];
    yiaddr.copy_from_slice(&frame[b + 16..b + 20]);
    let mut server = [0u8; 4];
    let mut router = [0u8; 4];
    let mut saw = false;
    let mut i = b + 240;
    while i + 1 < frame.len() {
        let t = frame[i];
        if t == 255 {
            break;
        }
        if t == 0 {
            i += 1;
            continue;
        }
        let l = frame[i + 1] as usize;
        if i + 2 + l > frame.len() {
            break;
        }
        let v = &frame[i + 2..i + 2 + l];
        if t == 53 && v.first() == Some(&msg) {
            saw = true;
        } else if t == 54 && l == 4 {
            server.copy_from_slice(v);
        } else if t == 3 && l >= 4 {
            router.copy_from_slice(&v[..4]);
        }
        i += 2 + l;
    }
    if !saw || yiaddr == [0, 0, 0, 0] {
        return None;
    }
    Some(Lease {
        yiaddr,
        server,
        router,
    })
}

fn ntp_frame(frame: &mut [u8], mac: &[u8; 6], gw: &[u8; 6], ip: &[u8; 4], dest: &[u8; 4]) -> usize {
    frame.fill(0);
    frame[..6].copy_from_slice(gw);
    frame[6..12].copy_from_slice(mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let udp_len = 8 + 48;
    let ip_len = 20 + udp_len;
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&(ip_len as u16).to_be_bytes());
    frame[22] = 64;
    frame[23] = 17;
    frame[26..30].copy_from_slice(ip);
    frame[30..34].copy_from_slice(dest);
    let sum = ip_checksum(&frame[14..34]);
    frame[24..26].copy_from_slice(&sum.to_be_bytes());
    frame[34..36].copy_from_slice(&12300u16.to_be_bytes());
    frame[36..38].copy_from_slice(&123u16.to_be_bytes());
    frame[38..40].copy_from_slice(&(udp_len as u16).to_be_bytes());
    frame[42] = 0x23;
    42 + 48
}

fn parse_ntp(frame: &[u8]) -> Option<u32> {
    if frame.len() < 42 + 48 || frame[12] != 0x08 || frame[13] != 0x00 || frame[23] != 17 {
        return None;
    }
    let src = u16::from_be_bytes([frame[34], frame[35]]);
    if src != 123 {
        return None;
    }
    let secs = u32::from_be_bytes([frame[82], frame[83], frame[84], frame[85]]);
    secs.checked_sub(NTP_DELTA)
}

fn ip_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < header.len() {
        sum += u32::from(u16::from_be_bytes([header[i], header[i + 1]]));
        i += 2;
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
