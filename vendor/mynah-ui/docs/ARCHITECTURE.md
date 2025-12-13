# MynahUI Architecture

## How do the Consumer and MynahUI work together?

Before how it works, it is better to clarify how to make it work. To consume MynahUI, there is only one single requirement, `npm` access. Adding `@aws/mynah-ui` to the `package.json` dependencies will allow the consumer to grab the desired version of MynahUI and allow them to create an instance of it to be rendered in the desired dom element.

To install:

```
npm install @aws/mynah-ui
```

And to create the instance:

```
const mynahUI = new MynahUI({...});
```

#### So, how is the flow between the consumer and the MynahUI in general?

As indicated above in the section, it expects data and sends events. The expected data from the MynahUI can be passed with several ways like defining them during the initialization, updating the data store directly or adding one or more chat items during the runtime. Let’s take a look to the basic flow between MynahUI and the consumer:

![image](https://github.com/user-attachments/assets/052ff1a4-e2f8-449f-a793-32dff333f6a5)

As we can clarify from the flow, MynahUI expects data from the consumer app and renders new elements or updates existing ones on the UI. And it is also responsible to deliver the user events to the consumer app to let them run their logic.




## How does MynahUI work under the hood?
![image](https://github.com/user-attachments/assets/f9ea537f-6db7-4249-b347-f46812646e7e)

MynahUI relies on three core structures, the **Data Store**, **Global Event Handler** and **Dom Builder**.  The combination of these 3 basically drives the MynahUI structure.


#### Let’s break down the **Data Store**:

The data store consists of 2 parts. The main data store holds all the current data for the tabs. Since MynahUI supports multiple tabs, each tab has its own data. And the second block in the data store is the data for each tab. 

Global Data Store → Tab Data Store 1, Tab Data Store 2 ... Tab Data Store N

Tab Data store holds every single content related with that tab, like chat items, tab title, background, prompt input field related information etc.

Here’s an overview of what it looks like:

![image](https://github.com/user-attachments/assets/f375031a-e2bb-4015-a3c1-ae88739b59cd)



#### Let’s break down the Global Event Handler:

The global event handler can be used by any component, to listen or fire a non data related event happened through the system. And more importantly, they can also fire events to inform the subscribers.
For example, when a tab gets focus (basically being selected) it fires an event through the global event system which is called `TAB_FOCUS`. And if there is any subscriber to that event, their attached handler function will be called. 

![image](https://github.com/user-attachments/assets/ea9157da-0030-4d85-8ede-4cbe918d6512)


#### Let’s break down the DomBuilder:

DomBuilder is at the heart of the rendering part of MynahUI. Basically, every single UI (HTML) element is being generated from the DomBuilder. It helps to manage dom manipulation from one single space. For example when you need to add some specific attribute to any dom generated in the MynahUI, it will be handled from this **singleton** class.

![image](https://github.com/user-attachments/assets/40ccab42-a64f-4120-95a1-57822add9f80)



The main class (MynahUI) handles all the creation of these core parts and the communication between them.

To clarify how all those structures work together, **a simplified flow can be showed as follows**:

![image](https://github.com/user-attachments/assets/f816ad36-4ad3-4e13-913e-d6afb9939a4f)



### How do components work?

Components are using the DomBuilder to build up their HTML elements. Or, they can also use other components as well.
Each component should have their `render`, which should also be just an HTMLElement or an ExtendedHTMLElement which is the output of the DomBuilder.
For the styling of the elements and components, MynahUI uses basic css structure. However to make it as clean as possible to read and generate proper hierarchies, we’re building the output css from SCSS. 


>But an important notice here, we’re trying to avoid using SCSS variables as much as possible and keep every possible thing as a CSS Custom property.


The styling of the components cannot have static values or inline values. With the support of the CSS custom properties, it is possible to theme it in every single detail like colors, paddings, sizings, fonts even animations and transitions.

**Here’s a general look of a component structure:**

![image](https://github.com/user-attachments/assets/d7de7181-2e0d-43c2-8118-fffa7cc36156)



