/*
* Test case: 17
* In ReactTypeScript file.
*/
// Import React and ReactDOM for creating components and rendering them
import React from 'react';
import ReactDOM from 'react-dom';

// Defining the functional component 'App' with TypeScript syntax
const App: React.FC = () => {
  // The JSX structure returned by the App component
  // This JSX will be rendered to the DOM
  return (
    <div>
      {/* Displaying "Hello World" text on the web page */}
      <h1>Hello World</h1>
    </div>
  );
};

// Rendering the App component into the root element in the HTML
// ReactDOM.render will attach the component to the actual DOM
ReactDOM.render(
  <App />, // The component to be rendered
  document.getElementById('root') // The DOM element where the component will be injected
);



