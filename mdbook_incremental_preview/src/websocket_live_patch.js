const web_socket = function() {
    const wsProtocol = location.protocol === 'https:' ? 'wss' : 'ws';
    const wsAddress = `${wsProtocol}://${location.host}/__mdbook_incremental_preview_live_patch`;
    return new WebSocket(wsAddress);
}();
web_socket.onopen = () => web_socket.send(location.pathname);
web_socket.onclose = () => location.reload();
web_socket.onmessage = (event) => {
    const contents = event.data;
    // NOTE: We assume that the contents are in <main>.
    document.querySelector('main').innerHTML = contents;
    // TODO: Reload JavaScript.
};

window.onbeforeunload = () => web_socket.close();
