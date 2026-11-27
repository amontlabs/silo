| case | stock | stock-ec2 | ec2 | on-main | sealed | explicit | closed | boundary | boundary-metadata | boundary-down |
|---|---|---|---|---|---|---|---|---|---|---|
| gateway-ports | 53,80,7790 | 53,80,7790 | 53,80 | **53,7790** (expected 53) | 53 | 53 | 53 | 53 | 53 | 53 |
| host-loopback-ports | 2024,2222,7777,7778,41205,45741 | 2024,2222,7777,7778,41205,45741 | 2024,2222,7777,7778,41205,45741 | none | none | none | none | none | none | none |
| gateway-unbound-tcp | **reached** (expected blocked) | **reached** (expected blocked) | blocked | **reached** (expected blocked) | blocked | blocked | blocked | blocked | blocked | blocked |
| api-list | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked | blocked |
| api-expose | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked | blocked |
| host-loopback-tcp | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked | blocked |
| host-own-address-tcp | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| lan-neighbour-tcp | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| lan-neighbour-udp | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| lan-neighbour-ping | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| host-own-address-ping | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| public-lan-neighbour-tcp | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| public-lan-neighbour-ping | reached | reached | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked |
| metadata-tcp | blocked | **blocked** (expected reached) | reached | blocked | blocked | blocked | blocked | blocked | reached | blocked |
| metadata-ping | **reached** (expected blocked) | reached | reached | blocked | blocked | blocked | blocked | blocked | blocked | blocked |
| zero-net-tcp | blocked | blocked | blocked | blocked | blocked | blocked | blocked | blocked | blocked | blocked |
| internet-tcp | reached | reached | reached | reached | reached | reached | blocked | reached | reached | blocked |
| internet-udp | reached | reached | reached | reached | reached | reached | blocked | blocked | blocked | blocked |
| internet-ping | reached | reached | reached | reached | reached | reached | blocked | blocked | blocked | blocked |
| dns-name | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | none | 203.0.113.10 | 203.0.113.10 | none |
| dns-exfil | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | 203.0.113.10 | none | none | none | none |
| host-port-opened-from-inside | yes | yes | yes | no | no | no | no | no | no | no |
| way-in | works | works | works | works | works | works | works | works | works | works |
| dns-exfil-seen-by | host-resolver | host-resolver | host-resolver | host-resolver | host-resolver | host-resolver | no | boundary | boundary | no |

6 case(s) differ from expected.txt
