declare const memory: {
  /** Create multiple new entities in the knowledge graph */
  create_entities(params: {
    entities: {
      /** The type of the entity */
      entityType: string;
      /** The name of the entity */
      name: string;
      /** An array of observation contents associated with the entity */
      observations: string[];
    }[];
  }): Promise<unknown>;
  /** Create multiple new relations between entities in the knowledge graph. Relations should be in active voice */
  create_relations(params: {
    relations: {
      /** The name of the entity where the relation starts */
      from: string;
      /** The type of the relation */
      relationType: string;
      /** The name of the entity where the relation ends */
      to: string;
    }[];
  }): Promise<unknown>;
  /** Add new observations to existing entities in the knowledge graph */
  add_observations(params: {
    observations: {
      /** An array of observation contents to add */
      contents: string[];
      /** The name of the entity to add the observations to */
      entityName: string;
    }[];
  }): Promise<unknown>;
  /** Delete multiple entities and their associated relations from the knowledge graph */
  delete_entities(params: {
    /** An array of entity names to delete */
    entityNames: string[];
  }): Promise<unknown>;
  /** Delete specific observations from entities in the knowledge graph */
  delete_observations(params: {
    deletions: {
      /** The name of the entity containing the observations */
      entityName: string;
      /** An array of observations to delete */
      observations: string[];
    }[];
  }): Promise<unknown>;
  /** Delete multiple relations from the knowledge graph */
  delete_relations(params: {
    /** An array of relations to delete */
    relations: {
      /** The name of the entity where the relation starts */
      from: string;
      /** The type of the relation */
      relationType: string;
      /** The name of the entity where the relation ends */
      to: string;
    }[];
  }): Promise<unknown>;
  /** Read the entire knowledge graph */
  read_graph(): Promise<unknown>;
  /** Search for nodes in the knowledge graph based on a query */
  search_nodes(params: {
    /** The search query to match against entity names, types, and observation content */
    query: string;
  }): Promise<unknown>;
  /** Open specific nodes in the knowledge graph by their names */
  open_nodes(params: {
    /** An array of entity names to retrieve */
    names: string[];
  }): Promise<unknown>;
};
