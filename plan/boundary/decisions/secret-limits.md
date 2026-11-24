# Can a secret be limited

## The choice

An agent's part, 22 November 2026. You give a fork a GitHub token so that it can read a private
repository, and you do not want it to push. A Git fetch over HTTPS is sent as a POST request.

The options:

- Reading means GET, plus listed reads (recommended). The token goes out with GET requests and with
  the requests a rule lists as reads for that host, such as a Git fetch. It can also have an end
  time. It costs a list to keep for each such host, and a wrong line in it lets a write through.
- Reading means GET only. It costs the case above: the fork cannot fetch the repository with a token
  limited to reading.
- No limit to reading in Silo. You make a read-only token at GitHub, and Silo keeps only the end
  time. It costs every service that offers no read-only token.

Recommended: reading means GET, plus listed reads. A limit that refuses a Git fetch would be
switched off on the first day.

## Which hosts send a read as a POST (24 November 2026)

An agent's part, 24 November 2026. For a secret limited to reads, these are the APIs where "GET
and HEAD" gets it wrong. Git and AWS were captured here: git 2.43 against a local smart HTTP
server, and the AWS CLI against the stand-in of `secret-placeholder`. The rest are from each
protocol's specification.

| Where | A read is sent as | A write is sent as | Silo tells them apart by |
| --- | --- | --- | --- |
| Git over HTTPS (github.com, gitlab.com, any Git host) | `GET …/info/refs?service=git-upload-pack`, then `POST …/git-upload-pack`, for a clone, a fetch or `ls-remote` alike | `GET …/info/refs?service=git-receive-pack`, then `POST …/git-receive-pack` | the path |
| AWS, Query protocol (STS, EC2, IAM, …) | `POST /`, form body `Action=GetCallerIdentity`, `Describe…`, `List…` | `POST /`, `Action=Run…`, `Create…`, `Delete…` | the body's `Action` |
| AWS, JSON protocol (DynamoDB, …) | `POST /`, header `X-Amz-Target: DynamoDB_20120810.ListTables`, `GetItem`, `Query` | the same with `PutItem`, `DeleteItem` | the `X-Amz-Target` header |
| GraphQL (api.github.com/graphql, GitLab, Linear, Shopify) | `POST /graphql`, a `query` | `POST /graphql`, a `mutation` | parsing the GraphQL document in the body: one document can hold several operations, picked by `operationName` |
| JSON-RPC (Ethereum nodes such as Infura and Alchemy) | `POST`, `"method": "eth_call"`, `eth_getBalance` | `POST`, `"method": "eth_sendRawTransaction"` | the body's `method`, for each call in a batch |
| Slack Web API | `POST /api/conversations.history` | `POST /api/chat.postMessage` | the path |
| Elasticsearch, OpenSearch | `POST /<index>/_search` | `POST /<index>/_doc` | the path |

- npm, PyPI, container registries, S3 and most REST APIs read with GET and write with another
  method, so they need no list.
- For GraphQL and JSON-RPC, the list has to read the body before the secret is swapped in, so the
  boundary holds that body until it has read it. These bodies are small.
- An unknown name fails closed. An AWS `Action`, a JSON-RPC method or a GraphQL field that the list
  does not know is treated as a write.
- Infura and Alchemy put the key in the URL's path (`/v3/<key>`), not in a header. The proposal in
  `secret-placeholder`'s notes, to swap only in declared credential headers, has to let a secret
  declare a place in the URL too.
