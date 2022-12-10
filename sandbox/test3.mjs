import http from "http";

const server = http.createServer();
server.listen(9180, "localhost");
server.on("request", x =>  console.log(x.headers));

const req = http.request("http://localhost:9180/");
req.setHeader("Expect", "100-continue");

req.on("information", x => console.log("information", x));
req.on("continue", () => console.log("continue"));
req.setHeader("X-Test", "test");
req.flushHeaders();
