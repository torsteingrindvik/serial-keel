// npm install ws
const WebSocket = require('ws');

const ws = new WebSocket('ws://127.0.0.1:3123/client');

ws.on('message', function incoming(data) {
    console.log(JSON.parse(data));
});

ws.on('open', function open() {
    const send = (o) => {
        ws.send(JSON.stringify(o));
    }

    send({ Control: { Mock: "/dev/ttyACM0" } });
    send({ Observe: { Mock: "/dev/ttyACM0" } });
    send({ Write: [{ Mock: "/dev/ttyACM0" }, "Hello\nWorld\nBye!"] });
});
