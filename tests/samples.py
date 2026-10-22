"""Share links for every supported protocol, shaped like the ones panels and providers hand out.
All credentials are throwaway; keys have valid formats so `sing-box check` accepts them."""

UUID = "11111111-1111-4111-8111-111111111111"
# A throwaway X25519 public key.
PUBKEY = "Vdp5mc4z6trF6_wM_BjEK2FeiZts-oO4wPUSPcdf2jc"

LINKS = {
    "vless-reality": f"vless://{UUID}@203.0.113.7:443?security=reality&pbk={PUBKEY}&sid=abcd"
                     "&sni=node.example.com&flow=xtls-rprx-vision&fp=chrome&type=tcp#reality",
    "vless-ws-tls": "vless://44444444-4444-4444-8444-444444444444@ws.example.com:443?encryption=none"
                    "&security=tls&sni=ws.example.com&type=ws&host=cdn.example.com&path=%2Fray%3Fed%3D2048#vless-ws",
    "vless-grpc-tls": "vless://55555555-5555-4555-8555-555555555555@198.51.100.3:8443?security=tls"
                      "&sni=grpc.example.com&type=grpc&serviceName=tunnel&alpn=h2#vless-grpc",
    "vmess-ws-tls": "vmess://eyJ2IjogIjIiLCAicHMiOiAidm0td3MiLCAiYWRkIjogInZtLmV4YW1wbGUuY29tIiwgInBvcnQiOiAiNDQzIiwg"
                    "ImlkIjogIjIyMjIyMjIyLTIyMjItNDIyMi04MjIyLTIyMjIyMjIyMjIyMiIsICJhaWQiOiAiMCIsICJzY3kiOiAiYXV0byIs"
                    "ICJuZXQiOiAid3MiLCAidHlwZSI6ICJub25lIiwgImhvc3QiOiAiY2RuLmV4YW1wbGUuY29tIiwgInBhdGgiOiAiL3JheSIs"
                    "ICJ0bHMiOiAidGxzIiwgInNuaSI6ICJjZG4uZXhhbXBsZS5jb20ifQ==",
    "vmess-tcp": "vmess://eyJ2IjogIjIiLCAicHMiOiAidm0tdGNwIiwgImFkZCI6ICIxOTguNTEuMTAwLjQiLCAicG9ydCI6IDEwMDg2LCAiaWQi"
                 "OiAiMzMzMzMzMzMtMzMzMy00MzMzLTgzMzMtMzMzMzMzMzMzMzMzIiwgImFpZCI6IDAsICJuZXQiOiAidGNwIiwgInR5cGUi"
                 "OiAibm9uZSIsICJ0bHMiOiAiIn0=",
    "trojan": "trojan://trojan-secret-pass@trojan.example.com:443?security=tls&sni=trojan.example.com&type=tcp#trojan",
    "trojan-ws": "trojan://trojan-ws-secret@198.51.100.5:443?sni=cdn.example.com&type=ws&path=%2Ftj#trojan-ws",
    "ss": "ss://YWVzLTI1Ni1nY206c3Mtc2VjcmV0LXBhc3M@198.51.100.2:8388#ss",
    "ss-2022": "ss://2022-blake3-aes-128-gcm:AAECAwQFBgcICQoLDA0ODw%3D%3D@ss22.example.com:8389#ss-2022",
    "ss-legacy": "ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpsZWdhY3ktc2VjcmV0QDE5OC41MS4xMDAuNjo4Mzg5#ss-legacy",
    "hysteria2": "hysteria2://hy2-secret-pass@hy.example.com:8443/?sni=hy.example.com&obfs=salamander"
                 "&obfs-password=obfs-secret-pass#hy2",
    "hy2-short": "hy2://hy2-other-secret@198.51.100.8?insecure=1&sni=www.example.com#hy2-short",
    "tuic": "tuic://66666666-6666-4666-8666-666666666666:tuic-secret-pass@198.51.100.9:443"
            "?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=tuic.example.com#tuic",
}

# What each link must turn into: (protocol, server, port, pf protocol).
EXPECTED = {
    "vless-reality": ("vless", "203.0.113.7", 443, "tcp"),
    "vless-ws-tls": ("vless", "ws.example.com", 443, "tcp"),
    "vless-grpc-tls": ("vless", "198.51.100.3", 8443, "tcp"),
    "vmess-ws-tls": ("vmess", "vm.example.com", 443, "tcp"),
    "vmess-tcp": ("vmess", "198.51.100.4", 10086, "tcp"),
    "trojan": ("trojan", "trojan.example.com", 443, "tcp"),
    "trojan-ws": ("trojan", "198.51.100.5", 443, "tcp"),
    "ss": ("shadowsocks", "198.51.100.2", 8388, "tcp"),
    "ss-2022": ("shadowsocks", "ss22.example.com", 8389, "tcp"),
    "ss-legacy": ("shadowsocks", "198.51.100.6", 8389, "tcp"),
    "hysteria2": ("hysteria2", "hy.example.com", 8443, "udp"),
    "hy2-short": ("hysteria2", "198.51.100.8", 443, "udp"),
    "tuic": ("tuic", "198.51.100.9", 443, "udp"),
}
