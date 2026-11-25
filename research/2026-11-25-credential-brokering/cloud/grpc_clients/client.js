// A Node gRPC client (@grpc/grpc-js) sending authorization: Bearer <token> three times.
const grpc = require("@grpc/grpc-js");
const [addr, tok] = process.argv.slice(2);
const c = new grpc.Client(addr, grpc.credentials.createInsecure());
const id = (b) => b;
let n = 0;
const one = () => {
  const md = new grpc.Metadata(); md.set("authorization", "Bearer " + tok);
  c.makeUnaryRequest("/demo.Echo/Ping", id, id, Buffer.from("ping"), md, (err) => {
    if (err) console.log("err", err.message);
    if (++n < 3) one(); else { console.log("done"); c.close(); }
  });
};
one();
