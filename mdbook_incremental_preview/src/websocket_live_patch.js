(function() {
    const wsProtocol = location.protocol === 'https:' ? 'wss' : 'ws';
    const wsAddress = `${wsProtocol}://${location.host}/__mdbook_incremental_preview_live_patch${location.pathname}`;
    const web_socket = new WebSocket(wsAddress);
    web_socket.onclose = () => location.reload();
    const contentElement = document.querySelector('main')
    web_socket.onmessage = (event) => {
        const content = event.data;
        // NOTE: We assume that the content is in <main>.
        contentElement.innerHTML = content;
        // TODO: Reload JavaScript.
    };

    window.onbeforeunload = () => web_socket.close();
})();
